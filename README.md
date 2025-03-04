# MiniJinja

[![Build Status](https://github.com/mitsuhiko/minijinja/workflows/Tests/badge.svg?branch=main)](https://github.com/mitsuhiko/minijinja/actions?query=workflow%3ATests)
[![License](https://img.shields.io/github/license/mitsuhiko/minijinja)](https://github.com/mitsuhiko/minijinja/blob/main/LICENSE)
[![Documentation](https://docs.rs/minijinja/badge.svg)](https://docs.rs/minijinja)

MiniJinja is a powerful but minimal dependency template engine for Rust which
is based on the syntax and behavior of the
[Jinja2](https://jinja.palletsprojects.com/) template engine for Python.

It's implemented on top of `serde` and only has a single dependency.  It supports
a range of features from Jinja2 including inheritance, filters and more.

```rust
use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct Context {
    name: String,
}

fn main() {
    let mut env = Environment::new();
    env.add_template("hello.txt", "Hello {{ name }}!").unwrap();
    let template = env.get_template("hello.txt").unwrap();
    println!("{}", template.render(&Context {
        name: "World".into()
    }).unwrap());
}
```