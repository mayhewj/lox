use std::collections::HashMap;
use std::mem;
use std::rc::Rc;

use parser::{BinOp, Expr, ExprNode, Function, LogicOp, Stmt, UnaryOp, Var};
use super::builtins::{Clock, Input};
use super::{Environment, Error, LoxCallable, LoxClass, LoxFunction, Result, Value};

pub struct Interpreter {
    env: Environment,
}

impl Default for Interpreter {
    fn default() -> Self {
        let env = Environment::default();
        env.define(Clock.name().into(), Value::Callable(Rc::new(Clock)));
        env.define(Input.name().into(), Value::Callable(Rc::new(Input)));
        Self { env }
    }
}

impl Interpreter {
    pub fn execute(&mut self, stmts: &[Stmt]) -> Result<()> {
        for stmt in stmts {
            self.exec_stmt(stmt)?;
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        match *stmt {
            Stmt::Expr(ref expr) => {
                self.evaluate(expr)?;
                Ok(())
            }
            Stmt::Print(ref expr) => {
                println!("{}", self.evaluate(expr)?);
                Ok(())
            }
            Stmt::VarDecl(ref identifier, ref initializer) => {
                let value = match *initializer {
                    Some(ref expr) => self.evaluate(expr)?,
                    None => Value::Nil,
                };

                self.env.define(identifier.name.clone(), value);
                Ok(())
            }
            Stmt::Fun(ref fun) => {
                let func = LoxFunction::new(Function::Decl(fun.clone()), self.env.clone(), false);
                self.env
                    .define(fun.name().into(), Value::Callable(Rc::new(func)));
                Ok(())
            }
            Stmt::Class(ref class) => {
                self.env.define(class.name().into(), Value::Nil);

                let mut superclass = None;

                if let Some(ref var) = class.superclass {
                    if let Value::Class(class) = self.lookup(var)? {
                        self.env = Environment::with_enclosing(self.env.clone());
                        self.env
                            .define("super".into(), Value::Class(Rc::clone(&class)));
                        superclass = Some(class);
                    } else {
                        err!(class.identifier.line, "Superclass must be a class");
                    }
                }

                let mut methods = HashMap::new();
                for method in &class.methods {
                    let func = LoxFunction::new(
                        Function::Decl(method.clone()),
                        self.env.clone(),
                        method.name() == "init",
                    );
                    methods.insert(method.name().into(), Rc::new(func));
                }

                if superclass.is_some() {
                    self.env = self.env.ancestor(1);
                }

                let lox_class = Rc::new(LoxClass::new(
                    class.name().into(),
                    superclass,
                    Rc::new(methods),
                ));

                self.env
                    .assign(class.name().into(), Value::Class(lox_class));
                Ok(())
            }
            Stmt::Return(ref expr, _) => {
                let value = if let Some(ref expr) = *expr {
                    self.evaluate(expr)?
                } else {
                    Value::Nil
                };

                Err(Error::Return(value))
            }
            Stmt::Block(ref stmts) => {
                let env = Environment::with_enclosing(self.env.clone());
                self.execute_block(stmts, env)
            }
            Stmt::If(ref condition, ref then_branch, ref else_branch) => {
                if self.evaluate(condition)?.is_truthy() {
                    self.exec_stmt(then_branch)?;
                } else if let Some(ref branch) = *else_branch {
                    self.exec_stmt(branch)?;
                }
                Ok(())
            }
            Stmt::While(ref condition, ref body) => {
                while self.evaluate(condition)?.is_truthy() {
                    self.exec_stmt(body)?;
                }
                Ok(())
            }
        }
    }

    fn evaluate(&mut self, node: &ExprNode) -> Result<Value> {
        match *node.expr() {
            Expr::Binary(ref left_expr, op, ref right_expr) => {
                let line = left_expr.line();
                let a = self.evaluate(left_expr)?;
                let b = self.evaluate(right_expr)?;

                match op {
                    BinOp::EqualEqual => Ok(Value::Bool(a == b)),
                    BinOp::BangEqual => Ok(Value::Bool(a != b)),
                    BinOp::Plus => match (a, b) {
                        (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                        (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
                        _ => err!(line, "Operands must be two numbers or two strings"),
                    },
                    BinOp::Minus => with_numbers(a, b, line, |a, b| Ok(Value::Number(a - b))),
                    BinOp::Star => with_numbers(a, b, line, |a, b| Ok(Value::Number(a * b))),
                    BinOp::Slash => with_numbers(a, b, line, |a, b| Ok(Value::Number(a / b))),
                    BinOp::Greater => with_numbers(a, b, line, |a, b| Ok(Value::Bool(a > b))),
                    BinOp::GreaterEqual => with_numbers(a, b, line, |a, b| Ok(Value::Bool(a >= b))),
                    BinOp::Less => with_numbers(a, b, line, |a, b| Ok(Value::Bool(a < b))),
                    BinOp::LessEqual => with_numbers(a, b, line, |a, b| Ok(Value::Bool(a <= b))),
                }
            }
            Expr::Logical(ref left_expr, op, ref right_expr) => {
                let left = self.evaluate(left_expr)?;

                match op {
                    LogicOp::And => if left.is_truthy() {
                        self.evaluate(right_expr)
                    } else {
                        Ok(left)
                    },
                    LogicOp::Or => if left.is_truthy() {
                        Ok(left)
                    } else {
                        self.evaluate(right_expr)
                    },
                }
            }
            Expr::Grouping(ref expr) => self.evaluate(expr),
            Expr::Literal(ref value) => Ok(value.clone()),
            Expr::Unary(op, ref expr) => {
                let value = self.evaluate(expr)?;

                match op {
                    UnaryOp::Minus => if let Value::Number(n) = value {
                        Ok(Value::Number(-n))
                    } else {
                        err!(expr.line(), "Operand must be a number")
                    },
                    UnaryOp::Bang => Ok(Value::Bool(!value.is_truthy())),
                }
            }
            Expr::Var(ref var) | Expr::This(ref var) => self.lookup(var),
            Expr::VarAssign(ref var, ref expr) => {
                let value = self.evaluate(expr)?;
                let env = self.env.ancestor(var.hops);

                if env.assign(var.name().into(), value.clone()) {
                    Ok(value)
                } else {
                    Err(Error::UndefinedVar(var.clone()))
                }
            }
            Expr::Call(ref callee, ref argument_exprs) => {
                let line = callee.line();
                let callee = self.evaluate(callee)?;

                let mut arguments = Vec::with_capacity(argument_exprs.len());
                for expr in argument_exprs {
                    arguments.push(self.evaluate(expr)?);
                }

                if let Some(f) = callee.into_callable() {
                    if f.arity() == arguments.len() {
                        f.call(self, arguments)
                    } else {
                        err!(
                            line,
                            "Expected {} arguments but got {}",
                            f.arity(),
                            arguments.len()
                        )
                    }
                } else {
                    err!(line, "Can only call functions and classes")
                }
            }
            Expr::Fun(ref fun) => {
                let callable =
                    LoxFunction::new(Function::Expr(fun.clone()), self.env.clone(), false);
                Ok(Value::Callable(Rc::new(callable)))
            }
            Expr::Get(ref expr, ref identifier) => {
                let value = self.evaluate(expr)?;

                if let Value::Instance(instance) = value {
                    if let Some(value) = instance.get_field(&identifier.name) {
                        Ok(value)
                    } else {
                        Ok(Value::Callable(instance
                            .class()
                            .bind_method(identifier, Rc::clone(&instance))?))
                    }
                } else {
                    err!(expr.line(), "Only instances have properties")
                }
            }
            Expr::Set(ref left, ref identifier, ref right) => {
                let object = self.evaluate(left)?;
                if let Value::Instance(instance) = object {
                    let value = self.evaluate(right)?;
                    instance.set(identifier.name.clone(), value.clone());
                    Ok(value)
                } else {
                    err!(left.line(), "Only instances have fields")
                }
            }
            Expr::Super(ref super_var, ref identifier) => {
                let env = self.env.ancestor(super_var.hops - 1);

                let instance = match env.get("this").unwrap() {
                    Value::Instance(instance) => instance,
                    value => panic!("Expected 'this' to be an Instance, found {:?}", value),
                };

                let superclass = match self.lookup(super_var)? {
                    Value::Class(class) => class,
                    value => panic!("Expected 'super' to be a Class, found {:?}", value),
                };

                Ok(Value::Callable(superclass
                    .bind_method(identifier, instance)?))
            }
        }
    }

    pub fn execute_block(&mut self, block: &[Stmt], env: Environment) -> Result<()> {
        let previous = mem::replace(&mut self.env, env);

        let mut result = Ok(());
        for stmt in block {
            result = self.exec_stmt(stmt);
            if result.is_err() {
                break;
            }
        }

        // Restore previous environment.
        self.env = previous;
        result
    }

    fn lookup(&self, var: &Var) -> Result<Value> {
        self.env
            .ancestor(var.hops)
            .get(var.name())
            .ok_or_else(|| Error::UndefinedVar(var.clone()))
    }
}

fn with_numbers<F>(left: Value, right: Value, line: usize, f: F) -> Result<Value>
where
    F: Fn(f64, f64) -> Result<Value>,
{
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => f(left, right),
        _ => err!(line, "Operands must be numbers"),
    }
}
