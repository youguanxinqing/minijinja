use crate::ast::{self, Spanned};
use crate::error::{Error, ErrorKind};
use crate::lexer::tokenize;
use crate::tokens::{Span, Token};
use crate::value::Value;

const RESERVED_NAMES: [&str; 7] = ["true", "True", "false", "False", "none", "None", "loop"];

macro_rules! syntax_error {
    ($msg:expr) => {{
        return Err(Error::new(ErrorKind::SyntaxError, $msg));
    }};
    ($msg:expr, $($tt:tt)*) => {{
        return Err(Error::new(ErrorKind::SyntaxError, format!($msg, $($tt)*)));
    }};
}

macro_rules! expect_token {
    ($parser:expr, $expectation:expr) => {{
        match $parser.stream.next()? {
            Some(rv) => Ok(rv),
            None => Err(Error::new(
                ErrorKind::SyntaxError,
                format!("unexpected end of input, expected {}", $expectation),
            )),
        }
    }};
    ($parser:expr, $match:pat, $expectation:expr) => {{
        match $parser.stream.next()? {
            Some((token, span)) if matches!(token, $match) => Ok((token, span)),
            Some((token, _)) => Err(Error::new(
                ErrorKind::SyntaxError,
                format!("unexpected {}, expected {}", token, $expectation),
            )),
            None => Err(Error::new(
                ErrorKind::SyntaxError,
                format!("unexpected end of input, expected {}", $expectation),
            )),
        }
    }};
    ($parser:expr, $match:pat => $target:expr, $expectation:expr) => {{
        match $parser.stream.next()? {
            Some(($match, span)) => Ok(($target, span)),
            Some((token, _)) => Err(Error::new(
                ErrorKind::SyntaxError,
                format!("unexpected {}, expected {}", token, $expectation),
            )),
            None => Err(Error::new(
                ErrorKind::SyntaxError,
                format!("unexpected end of input, expected {}", $expectation),
            )),
        }
    }};
}

struct TokenStream<'a> {
    iter: Box<dyn Iterator<Item = Result<(Token<'a>, Span), Error>> + 'a>,
    current: Option<Result<(Token<'a>, Span), Error>>,
    current_span: Span,
}

impl<'a> TokenStream<'a> {
    /// Tokenize a template
    pub fn new(source: &'a str, in_expr: bool) -> TokenStream<'a> {
        TokenStream {
            iter: (Box::new(tokenize(source, in_expr)) as Box<dyn Iterator<Item = _>>),
            current: None,
            current_span: Span::default(),
        }
    }

    /// Advance the stream.
    pub fn next(&mut self) -> Result<Option<(Token<'a>, Span)>, Error> {
        let rv = self.current.take();
        self.current = self.iter.next();
        if let Some(Ok((_, span))) = self.current {
            self.current_span = span;
        }
        rv.transpose()
    }

    /// Look at the current token
    pub fn current(&mut self) -> Result<Option<(&Token<'a>, Span)>, Error> {
        if self.current.is_none() {
            self.next()?;
        }
        match self.current {
            Some(Ok(ref tok)) => Ok(Some((&tok.0, tok.1))),
            Some(Err(_)) => Err(self.current.take().unwrap().unwrap_err()),
            None => Ok(None),
        }
    }

    /// Expands the span
    pub fn expand_span(&self, mut span: Span) -> Span {
        span.end_line = self.current_span.end_line;
        span.end_col = self.current_span.end_col;
        span
    }

    /// Returns the last seen span.
    pub fn current_span(&self) -> Span {
        self.current_span
    }
}

struct Parser<'a> {
    filename: &'a str,
    stream: TokenStream<'a>,
}

macro_rules! binop {
    ($func:ident, $next:ident, { $($tok:tt)* }) => {
        fn $func(&mut self) -> Result<ast::Expr<'a>, Error> {
            let span = self.stream.current_span();
            let mut left = self.$next()?;
            loop {
                let op = match self.stream.current()? {
                    $($tok)*
                    _ => break,
                };
                self.stream.next()?;
                let right = self.$next()?;
                left = ast::Expr::BinOp(Spanned::new(
                    ast::BinOp {
                        op,
                        left,
                        right,
                    },
                    self.stream.expand_span(span),
                ));
            }
            Ok(left)
        }
    };
}

macro_rules! unaryop {
    ($func:ident, $next:ident, { $($tok:tt)* }) => {
        fn $func(&mut self) -> Result<ast::Expr<'a>, Error> {
            let span = self.stream.current_span();
            let op = match self.stream.current()? {
                $($tok)*
                _ => return self.$next()
            };
            self.stream.next()?;
            return Ok(ast::Expr::UnaryOp(Spanned::new(
                ast::UnaryOp {
                    op,
                    expr: self.$func()?,
                },
                self.stream.expand_span(span),
            )));
        }
    };
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str, filename: &'a str, in_expr: bool) -> Parser<'a> {
        Parser {
            filename,
            stream: TokenStream::new(source, in_expr),
        }
    }

    binop!(parse_or, parse_and, {
        Some((Token::Ident("or"), _)) => ast::BinOpKind::ScOr,
    });
    binop!(parse_and, parse_not, {
        Some((Token::Ident("and"), _)) => ast::BinOpKind::ScAnd,
    });
    unaryop!(parse_not, parse_compare, {
        Some((Token::Ident("not"), _)) => ast::UnaryOpKind::Not,
    });

    fn parse_compare(&mut self) -> Result<ast::Expr<'a>, Error> {
        let mut span = self.stream.current_span();
        let mut expr = self.parse_math1()?;
        loop {
            let op = match self.stream.current()? {
                Some((Token::Eq, _)) => ast::BinOpKind::Eq,
                Some((Token::Ne, _)) => ast::BinOpKind::Ne,
                Some((Token::Lt, _)) => ast::BinOpKind::Lt,
                Some((Token::Lte, _)) => ast::BinOpKind::Lte,
                Some((Token::Gt, _)) => ast::BinOpKind::Gt,
                Some((Token::Gte, _)) => ast::BinOpKind::Gte,
                _ => break,
            };
            self.stream.next()?;
            expr = ast::Expr::BinOp(Spanned::new(
                ast::BinOp {
                    op,
                    left: expr,
                    right: self.parse_math1()?,
                },
                self.stream.expand_span(span),
            ));
            span = self.stream.current_span();
        }
        Ok(expr)
    }

    binop!(parse_math1, parse_concat, {
        Some((Token::Plus, _)) => ast::BinOpKind::Add,
        Some((Token::Minus, _)) => ast::BinOpKind::Sub,
    });
    binop!(parse_concat, parse_math2, {
        Some((Token::Tilde, _)) => ast::BinOpKind::Concat,
    });
    binop!(parse_math2, parse_pow, {
        Some((Token::Mul, _)) => ast::BinOpKind::Mul,
        Some((Token::Div, _)) => ast::BinOpKind::Div,
        Some((Token::FloorDiv, _)) => ast::BinOpKind::FloorDiv,
        Some((Token::Mod, _)) => ast::BinOpKind::Rem,
    });
    binop!(parse_pow, parse_unary, {
        Some((Token::Pow, _)) => ast::BinOpKind::Pow,
    });
    unaryop!(parse_unary_only, parse_primary, {
        Some((Token::Minus, _)) => ast::UnaryOpKind::Neg,
    });

    fn parse_unary(&mut self) -> Result<ast::Expr<'a>, Error> {
        let mut expr = self.parse_unary_only()?;
        expr = self.parse_postfix(expr)?;
        self.parse_filter_expr(expr)
    }

    fn parse_postfix(&mut self, expr: ast::Expr<'a>) -> Result<ast::Expr<'a>, Error> {
        let mut expr = expr;
        loop {
            match self.stream.current()? {
                Some((Token::Dot, span)) => {
                    self.stream.next()?;
                    let (name, _) = expect_token!(self, Token::Ident(name) => name, "identifier")?;
                    expr = ast::Expr::GetAttr(Spanned::new(
                        ast::GetAttr { name, expr },
                        self.stream.expand_span(span),
                    ));
                }
                Some((Token::BracketOpen, span)) => {
                    self.stream.next()?;
                    let subscript_expr = self.parse_expr()?;
                    expect_token!(self, Token::BracketClose, "`]`")?;
                    expr = ast::Expr::GetItem(Spanned::new(
                        ast::GetItem {
                            expr,
                            subscript_expr,
                        },
                        self.stream.expand_span(span),
                    ));
                }
                Some((Token::ParenOpen, span)) => {
                    let args = self.parse_args()?;
                    expr = ast::Expr::Call(Spanned::new(
                        ast::Call { expr, args },
                        self.stream.expand_span(span),
                    ));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_filter_expr(&mut self, expr: ast::Expr<'a>) -> Result<ast::Expr<'a>, Error> {
        let mut expr = expr;
        loop {
            match self.stream.current()? {
                Some((Token::Pipe, _)) => {
                    self.stream.next()?;
                    let (name, span) =
                        expect_token!(self, Token::Ident(name) => name, "identifier")?;
                    let args = if matches!(self.stream.current()?, Some((Token::ParenOpen, _))) {
                        self.parse_args()?
                    } else {
                        Vec::new()
                    };
                    expr = ast::Expr::Filter(Spanned::new(
                        ast::Filter { name, expr, args },
                        self.stream.expand_span(span),
                    ));
                }
                Some((Token::Ident("is"), _)) => {
                    self.stream.next()?;
                    let (name, span) =
                        expect_token!(self, Token::Ident(name) => name, "identifier")?;
                    let args = if matches!(self.stream.current()?, Some((Token::ParenOpen, _))) {
                        self.parse_args()?
                    } else {
                        Vec::new()
                    };
                    expr = ast::Expr::Test(Spanned::new(
                        ast::Test { name, expr, args },
                        self.stream.expand_span(span),
                    ));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<ast::Expr<'a>>, Error> {
        let mut args = Vec::new();
        expect_token!(self, Token::ParenOpen, "`(`")?;
        loop {
            if matches!(self.stream.current()?, Some((Token::ParenClose, _))) {
                break;
            }
            if !args.is_empty() {
                expect_token!(self, Token::Comma, "`,`")?;
            }
            args.push(self.parse_expr()?);
        }
        expect_token!(self, Token::ParenClose, "`)`")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<ast::Expr<'a>, Error> {
        let (token, span) = expect_token!(self, "expression")?;
        macro_rules! const_val {
            ($expr:expr) => {
                ast::Expr::Const(Spanned::new(
                    ast::Const {
                        value: Value::from($expr),
                    },
                    span,
                ))
            };
        }

        match token {
            Token::Ident("true") | Token::Ident("True") => Ok(const_val!(true)),
            Token::Ident("false") | Token::Ident("False") => Ok(const_val!(false)),
            Token::Ident("none") | Token::Ident("None") => Ok(const_val!(())),
            Token::Ident(name) => Ok(ast::Expr::Var(Spanned::new(ast::Var { id: name }, span))),
            Token::Str(val) => Ok(const_val!(val)),
            Token::Int(val) => Ok(const_val!(val)),
            Token::Float(val) => Ok(const_val!(val)),
            Token::ParenOpen => {
                let expr = self.parse_expr()?;
                expect_token!(self, Token::ParenClose, "`)`")?;
                Ok(expr)
            }
            Token::BracketOpen => {
                let mut items = Vec::new();
                loop {
                    if matches!(self.stream.current()?, Some((Token::BracketClose, _))) {
                        break;
                    }
                    if !items.is_empty() {
                        expect_token!(self, Token::Comma, "`,`")?;
                    }
                    items.push(self.parse_expr()?);
                }
                expect_token!(self, Token::BracketClose, "`]`")?;
                Ok(ast::Expr::List(Spanned::new(
                    ast::List { items },
                    self.stream.expand_span(span),
                )))
            }
            Token::BraceOpen => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                loop {
                    if matches!(self.stream.current()?, Some((Token::BraceClose, _))) {
                        break;
                    }
                    if !keys.is_empty() {
                        expect_token!(self, Token::Comma, "`,`")?;
                    }
                    keys.push(self.parse_expr()?);
                    expect_token!(self, Token::Colon, "`:`")?;
                    values.push(self.parse_expr()?);
                }
                expect_token!(self, Token::BraceClose, "`]`")?;
                Ok(ast::Expr::Map(Spanned::new(
                    ast::Map { keys, values },
                    self.stream.expand_span(span),
                )))
            }
            token => syntax_error!("unexpected {}", token),
        }
    }

    pub fn parse_expr(&mut self) -> Result<ast::Expr<'a>, Error> {
        self.parse_or()
    }

    fn parse_stmt(&mut self) -> Result<ast::Stmt<'a>, Error> {
        let (token, span) = expect_token!(self, "block keyword")?;
        match token {
            Token::Ident("for") => Ok(ast::Stmt::ForLoop(Spanned::new(
                self.parse_for_stmt()?,
                self.stream.expand_span(span),
            ))),
            Token::Ident("if") => Ok(ast::Stmt::IfCond(Spanned::new(
                self.parse_if_cond()?,
                self.stream.expand_span(span),
            ))),
            Token::Ident("with") => Ok(ast::Stmt::WithBlock(Spanned::new(
                self.parse_with_block()?,
                self.stream.expand_span(span),
            ))),
            Token::Ident("block") => Ok(ast::Stmt::Block(Spanned::new(
                self.parse_block()?,
                self.stream.expand_span(span),
            ))),
            Token::Ident("extends") => Ok(ast::Stmt::Extends(Spanned::new(
                self.parse_extends()?,
                self.stream.expand_span(span),
            ))),
            Token::Ident("autoescape") => Ok(ast::Stmt::AutoEscape(Spanned::new(
                self.parse_auto_escape()?,
                self.stream.expand_span(span),
            ))),
            _ => syntax_error!("unknown block"),
        }
    }

    fn parse_assign_target(&mut self) -> Result<&'a str, Error> {
        let (target, _) = expect_token!(self, Token::Ident(name) => name, "identifier")?;
        if RESERVED_NAMES.contains(&target) {
            syntax_error!("cannot assign to reserved variable name {}", target);
        }
        Ok(target)
    }

    fn parse_for_stmt(&mut self) -> Result<ast::ForLoop<'a>, Error> {
        let target = self.parse_assign_target()?;
        expect_token!(self, Token::Ident("in"), "in")?;
        let iter = self.parse_expr()?;
        expect_token!(self, Token::BlockEnd(..), "end of block")?;
        let body = self.subparse(|tok| matches!(tok, Token::Ident("endfor")))?;
        self.stream.next()?;
        Ok(ast::ForLoop { target, iter, body })
    }

    fn parse_if_cond(&mut self) -> Result<ast::IfCond<'a>, Error> {
        let expr = self.parse_expr()?;
        expect_token!(self, Token::BlockEnd(..), "end of block")?;
        let true_body = self.subparse(|tok| {
            matches!(
                tok,
                Token::Ident("endif") | Token::Ident("else") | Token::Ident("elif")
            )
        })?;
        let false_body = match self.stream.next()? {
            Some((Token::Ident("else"), _)) => {
                expect_token!(self, Token::BlockEnd(..), "end of block")?;
                let rv = self.subparse(|tok| matches!(tok, Token::Ident("endif")))?;
                self.stream.next()?;
                rv
            }
            Some((Token::Ident("elif"), span)) => vec![ast::Stmt::IfCond(Spanned::new(
                self.parse_if_cond()?,
                self.stream.expand_span(span),
            ))],
            _ => Vec::new(),
        };

        Ok(ast::IfCond {
            expr,
            true_body,
            false_body,
        })
    }

    fn parse_with_block(&mut self) -> Result<ast::WithBlock<'a>, Error> {
        let mut assignments = Vec::new();

        while !matches!(self.stream.current()?, Some((Token::BlockEnd(_), _))) {
            if !assignments.is_empty() {
                expect_token!(self, Token::Comma, "comma")?;
            }
            let target = self.parse_assign_target()?;
            expect_token!(self, Token::Assign, "assignment operator")?;
            let expr = self.parse_expr()?;
            assignments.push((target, expr));
        }

        expect_token!(self, Token::BlockEnd(..), "end of block")?;
        let body = self.subparse(|tok| matches!(tok, Token::Ident("endwith")))?;
        self.stream.next()?;
        Ok(ast::WithBlock { assignments, body })
    }

    fn parse_block(&mut self) -> Result<ast::Block<'a>, Error> {
        let (name, _) = expect_token!(self, Token::Ident(name) => name, "identifier")?;
        expect_token!(self, Token::BlockEnd(..), "end of block")?;
        let body = self.subparse(|tok| matches!(tok, Token::Ident("endblock")))?;
        self.stream.next()?;

        if let Some((Token::Ident(trailing_name), _)) = self.stream.current()? {
            if *trailing_name != name {
                syntax_error!(
                    "mismatching name on block. Got `{}`, expected `{}`",
                    *trailing_name,
                    name
                );
            }
            self.stream.next()?;
        }

        Ok(ast::Block { name, body })
    }

    fn parse_extends(&mut self) -> Result<ast::Extends<'a>, Error> {
        let name = self.parse_expr()?;
        Ok(ast::Extends { name })
    }

    fn parse_auto_escape(&mut self) -> Result<ast::AutoEscape<'a>, Error> {
        let enabled = self.parse_expr()?;
        expect_token!(self, Token::BlockEnd(..), "end of block")?;
        let body = self.subparse(|tok| matches!(tok, Token::Ident("endautoescape")))?;
        self.stream.next()?;
        Ok(ast::AutoEscape { enabled, body })
    }

    fn subparse<F: FnMut(&Token) -> bool>(
        &mut self,
        mut end_check: F,
    ) -> Result<Vec<ast::Stmt<'a>>, Error> {
        let mut rv = Vec::new();
        while let Some((token, span)) = self.stream.next()? {
            match token {
                Token::TemplateData(raw) => {
                    rv.push(ast::Stmt::EmitRaw(Spanned::new(ast::EmitRaw { raw }, span)))
                }
                Token::VariableStart(_) => {
                    let expr = self.parse_expr()?;
                    rv.push(ast::Stmt::EmitExpr(Spanned::new(
                        ast::EmitExpr { expr },
                        self.stream.expand_span(span),
                    )));
                    expect_token!(self, Token::VariableEnd(..), "end of variable block")?;
                }
                Token::BlockStart(_) => {
                    let (tok, _span) = match self.stream.current()? {
                        Some(rv) => rv,
                        None => syntax_error!("unexpected end of input, expected keyword"),
                    };
                    if end_check(tok) {
                        return Ok(rv);
                    }
                    rv.push(self.parse_stmt()?);
                    expect_token!(self, Token::BlockEnd(..), "end of block")?;
                }
                _ => unreachable!("lexer produced garbage"),
            }
        }
        Ok(rv)
    }

    pub fn parse(&mut self) -> Result<ast::Stmt<'a>, Error> {
        // start the stream
        self.stream.next()?;
        let span = self.stream.current_span();
        Ok(ast::Stmt::Template(Spanned::new(
            ast::Template {
                children: self.subparse(|_| false)?,
            },
            self.stream.expand_span(span),
        )))
    }
}

/// Parses a template
pub fn parse<'a>(source: &'a str, filename: &'a str) -> Result<ast::Stmt<'a>, Error> {
    let mut parser = Parser::new(source, filename, false);
    parser.parse().map_err(|mut err| {
        if err.line().is_none() {
            err.set_location(parser.filename, parser.stream.current_span().start_line)
        }
        err
    })
}

/// Parses an expression
pub fn parse_expr(source: &str) -> Result<ast::Expr<'_>, Error> {
    let mut parser = Parser::new(source, "<expression>", true);
    parser.parse_expr().map_err(|mut err| {
        if err.line().is_none() {
            err.set_location(parser.filename, parser.stream.current_span().start_line)
        }
        err
    })
}
