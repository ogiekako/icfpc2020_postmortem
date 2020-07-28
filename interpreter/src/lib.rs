#![allow(unused)]

extern crate anyhow;
extern crate console_error_panic_hook;

extern crate itertools;
extern crate lazy_static;
extern crate serde;
extern crate typed_arena;
extern crate wasm_bindgen;

#[cfg(target_os = "linux")]
extern crate reqwest;

use lazy_static::lazy_static;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    rc::Rc,
    sync::Mutex,
};
use typed_arena::Arena;
use wasm_bindgen::prelude::*;

lazy_static! {
    static ref API_KEY: Mutex<String> = Mutex::new(std::env::var("API_KEY").unwrap_or(String::new()));
    static ref STR_PRIMITIVE: HashMap<&'static str, Primitive> = {
        use Primitive::*;
        let mut m = HashMap::new();
        m.insert("add", Add);
        m.insert("mul", Mul);
        m.insert("div", Div);
        m.insert("eq", Eq);
        m.insert("lt", Lt);
        m.insert("neg", Neg);
        m.insert("s", S);
        m.insert("c", C);
        m.insert("b", B);
        m.insert("i", I);
        m.insert("f", F);
        m.insert("t", T);
        m.insert("cons", Cons);
        m.insert("car", Car);
        m.insert("cdr", Cdr);
        m.insert("nil", Nil);
        m.insert("isnil", Isnil);
        m
    };
    static ref PRIMITIVE_STR: HashMap<Primitive, &'static str> = {
        let mut res = HashMap::new();
        STR_PRIMITIVE.iter().for_each(|(k, v)| {
            res.insert(*v, *k);
        });
        res
    };
}
#[derive(Clone, Eq, PartialEq, Debug)]
enum Expr {
    Ap(CachedExpr, CachedExpr),
    Op(Primitive, Vec<CachedExpr>),
    Num(i64),
    Var(String),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Primitive {
    Add,
    Mul,
    Div,
    Eq,
    Lt,
    Neg,
    S,
    C,
    B,
    I,
    F,
    T,
    Cons,
    Car,
    Cdr,
    Nil,
    Isnil,
}

impl Display for Primitive {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", PRIMITIVE_STR.get(self).unwrap())
    }
}

use Expr::*;

impl Into<CachedExpr> for Expr {
    fn into(self) -> CachedExpr {
        CachedExpr {
            expr: Rc::new(RefCell::new(self)),
            cached: Rc::new(RefCell::new(false)),
        }
    }
}

impl Expr {
    fn boolean(b: bool) -> Expr {
        if b {
            Op(Primitive::T, vec![])
        } else {
            Op(Primitive::F, vec![])
        }
    }
    fn eval(self, env: &Env) -> Expr {
        use Primitive::*;

        match self {
            Ap(l, r) => match &*l.eval(env) {
                Op(name, v) => {
                    let mut v = v.clone();
                    v.push(r.clone());
                    Op(*name, v)
                }
                _ => panic!("not op or func l: {:?}", l),
            }
            .eval(env),
            Var(name) => env.get(&name).unwrap().clone().eval(env),
            Op(name, v) => match (name, &v.as_slice()) {
                (B, [x0, x1, x2]) => Ap(x0.clone(), Ap(x1.clone(), x2.clone()).into()).eval(env),
                (C, [x0, x1, x2]) => Ap(Ap(x0.clone(), x2.clone()).into(), x1.clone()).eval(env),
                (S, [x0, x1, x2]) => Ap(Ap(x0.clone(), x2.clone()).into(), Ap(x1.clone(), x2.clone()).into()).eval(env),

                (I, [x0]) => x0.eval(env).clone(),
                (Car, [x2]) => Ap(x2.clone(), Expr::boolean(true).into()).eval(env),
                (Cdr, [x2]) => Ap(x2.clone(), Expr::boolean(false).into()).eval(env),
                (Neg, [x]) => Num(-x.eval(env).must_num()),
                (Nil, [x0]) => Expr::boolean(true).eval(env),
                (Isnil, [x0]) => match &*x0.eval(env) {
                    Op(Nil, v) if v.len() == 0 => Expr::boolean(true).eval(env),
                    Op(Cons, v) if v.len() == 2 => Expr::boolean(false).eval(env),
                    _ => panic!("unexpected x0: {:?}", x0),
                },
                (F, [x0, x1]) => x1.eval(env).clone(),
                (T, [x0, x1]) => x0.eval(env).clone(),

                (Add, [x, y]) => Num(x.eval(env).must_num() + y.eval(env).must_num()),
                (Mul, [x, y]) => Num(x.eval(env).must_num() * y.eval(env).must_num()),
                (Div, [x, y]) => Num(x.eval(env).must_num() / y.eval(env).must_num()),
                (Eq, [x, y]) => Expr::boolean(x.eval(env).must_num() == y.eval(env).must_num()),
                (Lt, [x, y]) => Expr::boolean(x.eval(env).must_num() < y.eval(env).must_num()),

                (Cons, [x0, x1, x2]) => Ap(Ap(x2.clone(), x0.clone()).into(), x1.clone()).eval(env),

                _ => Op(name, v),
            },
            _ => self,
        }
    }

    fn must_num(&self) -> i64 {
        match self {
            Expr::Num(x) => *x,
            _ => panic!("not a num: {}", self),
        }
    }
    fn must_op(&self) -> (&Primitive, &[CachedExpr]) {
        match self {
            Op(s, v) => (s, v.as_slice()),
            _ => panic!("not op"),
        }
    }
    fn must_list_rev(self, env: &Env) -> Vec<Expr> {
        let e = self.eval(env);
        match e.must_op() {
            (Primitive::Nil, []) => vec![],
            (Primitive::Cons, [x0, x1]) => {
                let mut res = x1.eval(&env).clone().must_list_rev(env);
                res.push(x0.expr.borrow().clone());
                res
            }
            _ => panic!("not list"),
        }
    }
    fn must_list(self, env: &Env) -> Vec<Expr> {
        self.must_list_rev(env).into_iter().rev().collect()
    }
    fn must_point(self, env: &Env) -> (i64, i64) {
        let e = self.eval(env);
        match e.must_op() {
            (Primitive::Cons, [x, y]) => {
                let x = x.eval(env);
                let y = y.eval(env);
                (x.must_num(), y.must_num())
            }
            _ => panic!("not vec"),
        }
    }
    fn cons(hd: CachedExpr, tl: CachedExpr) -> Expr {
        Ap(Ap(Op(Primitive::Cons, vec![]).into(), hd).into(), tl)
    }
    fn nil() -> Expr {
        Op(Primitive::Nil, vec![])
    }

    fn demod(self, env: &Env) -> Expr {
        Expr::demodulate(&self.modulate(env))
    }

    fn modulate(self, env: &Env) -> String {
        let e = self.eval(env);

        match e {
            Num(n) => {
                let mut res = String::new();
                let n = if n >= 0 {
                    res.push_str("01");
                    n
                } else {
                    res.push_str("10");
                    n.abs()
                };

                let keta = 64 - n.leading_zeros();
                let t = (keta + 3) / 4;

                for _ in 0..t {
                    res.push('1');
                }
                res.push('0');

                for i in (0..4 * t).rev() {
                    res.push(if (n >> i & 1) == 1 { '1' } else { '0' });
                }
                res
            }
            _ => match e.must_op() {
                (Primitive::Nil, []) => "00".into(),
                (Primitive::Cons, [hd, tl]) => "11".to_string() + &hd.eval(env).clone().modulate(env) + &tl.expr.borrow().clone().modulate(env),
                _ => panic!("unexpected op {}", e),
            },
        }
    }
    fn demodulate(s: &str) -> Expr {
        Expr::demodulate_iter(&mut s.chars().map(|c| c == '1'))
    }
    fn demodulate_iter(it: &mut impl Iterator<Item = bool>) -> Expr {
        let t0 = it.next().unwrap();
        let t1 = it.next().unwrap();

        match (t0, t1) {
            (false, false) => Expr::nil(),
            (true, true) => {
                let x = Expr::demodulate_iter(it);
                let y = Expr::demodulate_iter(it);
                Expr::cons(x.into(), y.into())
            }
            (_, pos) => {
                let mut t = 0;
                while it.next().unwrap() {
                    t += 1;
                }
                let mut v = 0;
                for i in (0..4 * t).rev() {
                    v |= (if it.next().unwrap() { 1 } else { 0 }) << i;
                }
                Num(if pos { v } else { -v })
            }
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
struct CachedExpr {
    expr: Rc<RefCell<Expr>>,
    cached: Rc<RefCell<bool>>,
}

type Env = HashMap<String, Expr>;

fn default_env() -> Env {
    let mut env = Env::new();

    for line in include_str!("../galaxy.txt").split("\n") {
        let ss = line.split(" = ").collect::<Vec<_>>();
        let (name, expr) = (ss[0], ss[1]);
        let e = parse_string(&env, expr);
        env.insert(name.to_string(), e);
    }

    env
}

impl CachedExpr {
    fn eval(&self, env: &Env) -> std::cell::Ref<Expr> {
        if self.cached.replace(true) {
            return self.expr.borrow();
        }
        let expr = self.expr.borrow().clone().eval(env);
        self.expr.replace(expr.clone());
        self.expr.borrow()
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Ap(l, r) => write!(f, "ap {} {}", l.expr.borrow(), r.expr.borrow()),
            Expr::Op(s, v) => {
                let mut res = format!("{}", s);
                for e in v.iter() {
                    res = format!("ap {} {}", res, e.expr.borrow());
                }
                write!(f, "{}", res)
            }
            Expr::Num(i) => write!(f, "{}", i),
            Expr::Var(s) => write!(f, "{}", s),
        }
    }
}

fn parse_string(env: &Env, expr: &str) -> Expr {
    parse(env, &mut expr.split(" ").map(String::from).into_iter().peekable())
}

fn parse(env: &Env, mut it: &mut std::iter::Peekable<impl std::iter::Iterator<Item = String>>) -> Expr {
    use Expr::*;

    let s = it.next().expect("iterator exhausted");
    match s.as_str() {
        "(" => {
            let mut lst = vec![];
            loop {
                if it.peek().unwrap() == ")" {
                    it.next().unwrap();
                    break;
                }
                lst.push(parse(env, it));
                if it.peek().unwrap() == "," {
                    it.next().unwrap();
                }
            }
            let mut res = Expr::nil();
            for x in lst.into_iter().rev() {
                res = Expr::cons(x.into(), res.into());
            }
            res
        }
        "ap" => Ap(parse(env, &mut it).into(), parse(env, &mut it).into()),
        "add" | "b" | "c" | "car" | "cdr" | "cons" | "div" | "eq" | "i" | "isnil" | "lt" | "f" | "mod" | "dem" | "vec" | "mul" | "neg" | "nil" | "s" | "t" => {
            if s == "vec" {
                Op(Primitive::Cons, vec![]).into()
            } else {
                Op(*STR_PRIMITIVE.get(s.as_str()).unwrap(), vec![]).into()
            }
        }
        s => {
            if let Ok(i) = s.parse::<i64>() {
                Num(i)
            } else if env.contains_key(s) || s.chars().next().unwrap() == ':' || s.chars().next().unwrap() == 'x' {
                Var(s.to_string())
            } else {
                panic!("unknown var {}", s);
            }
        }
    }
}

#[wasm_bindgen]
pub struct InteractResult {
    state: String,
    images: Vec<Vec<(i64, i64)>>,
}

#[wasm_bindgen]
impl InteractResult {
    pub fn state(&self) -> String {
        self.state.clone()
    }
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
    pub fn image(&self, i: usize) -> Image {
        Image {
            img: self.images[i].iter().map(|p| Point { x: p.0 as _, y: p.1 as _ }).collect(),
        }
    }
}

#[wasm_bindgen]
pub struct Image {
    img: Vec<Point>,
}

#[wasm_bindgen]
impl Image {
    pub fn count(&self) -> usize {
        self.img.len()
    }
    pub fn point(&self, i: usize) -> Point {
        self.img[i]
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[wasm_bindgen]
pub struct G {
    env: Env,
}

#[wasm_bindgen]
impl G {
    pub fn new() -> G {
        G { env: default_env() }
    }
    pub fn galaxy(&self, mut state: String, x: i32, y: i32, api_key: &str) -> InteractResult {
        self.interact("galaxy", state, x, y, api_key)
    }
    fn interact(&self, protocol: &str, mut state: String, x: i32, y: i32, api_key: &str) -> InteractResult {
        let env = &self.env;
        let mut vector = format!("ap ap vec {} {}", x, y);
        loop {
            let input = format!("ap ap {} {} {}", protocol, state, vector);
            let expr = parse_string(&self.env, &input);
            let (flag, new_state, data) = {
                let e = expr.eval(&self.env);
                let mut v = e.must_list(&self.env);
                (v.remove(0), v.remove(0), v.remove(0))
            };

            state = format!("{}", new_state.demod(env));
            match flag.must_num() {
                0 => {
                    return InteractResult {
                        state,
                        images: data
                            .must_list(env)
                            .into_iter()
                            .map(|l| l.must_list(env).into_iter().map(|v| v.must_point(env)).collect::<Vec<_>>())
                            .map(|mut l| {
                                l.sort();
                                l
                            })
                            .collect(),
                    }
                }
                1 => {
                    let next_data = send(&data, env, api_key);
                    vector = format!("{}", next_data);
                }
                _ => panic!("unexpected flag: {}", flag),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;

    #[test]
    fn test_statelessdraw() {
        let mut g = G::new();
        g.env.insert(
            "statelessdraw".into(),
            parse_string(
                &g.env,
                "ap ap c ap ap b b ap ap b ap b ap cons 0 ap ap c ap ap b b cons ap ap c cons nil ap ap c ap ap b cons ap ap c cons nil nil",
            ),
        );

        let res = g.interact("statelessdraw", "nil".into(), 1, 0, "");
        assert_eq!(res.state, "nil");
        assert_eq!(res.images, vec![vec![(1, 0)]]);
    }

    #[test]
    fn test_galaxy() {
        let g = G::new();
        for tc in vec![
            (
                "nil",
                (0, 0),
                "ap ap cons 0 ap ap cons ap ap cons 0 nil ap ap cons 0 ap ap cons nil nil",
                vec![
                    vec![
                        (-3, 0),
                        (-3, 1),
                        (-2, -1),
                        (-2, 2),
                        (-1, -3),
                        (-1, -1),
                        (-1, 0),
                        (-1, 3),
                        (0, -3),
                        (0, -1),
                        (0, 1),
                        (0, 3),
                        (1, -3),
                        (1, 0),
                        (1, 1),
                        (1, 3),
                        (2, -2),
                        (2, 1),
                        (3, -1),
                        (3, 0),
                    ],
                    vec![(-8, -2), (-7, -3)],
                    vec![],
                ],
            ),
            (
                "ap ap cons 0 ap ap cons ap ap cons 0 nil ap ap cons 0 ap ap cons nil nil",
                (0, 0),
                "ap ap cons 0 ap ap cons ap ap cons 1 nil ap ap cons 0 ap ap cons nil nil",
                vec![
                    vec![
                        (-3, 0),
                        (-3, 1),
                        (-2, -1),
                        (-2, 2),
                        (-1, -3),
                        (-1, -1),
                        (-1, 0),
                        (-1, 3),
                        (0, -3),
                        (0, -1),
                        (0, 1),
                        (0, 3),
                        (1, -3),
                        (1, 0),
                        (1, 1),
                        (1, 3),
                        (2, -2),
                        (2, 1),
                        (3, -1),
                        (3, 0),
                    ],
                    vec![(-8, -2), (-7, -3), (-7, -2)],
                    vec![],
                ],
            ),
        ] {
            let res = g.galaxy(tc.0.to_string(), (tc.1).0, (tc.1).1, API_KEY.lock().unwrap().as_str());
            assert_eq!(res.state, tc.2);
            assert_eq!(res.images, tc.3);
        }
    }

    #[test]
    fn test() {
        for tc in vec![
            ("ap ap add 1 2", "3"),
            ("ap ap add 1 ap ap add 2 3", "6"),
            ("ap ap t 1 :1", "1"),
            ("ap ap f :1 1", "1"),
            ("( )", "nil"),
            ("( 1 , 2 )", "ap ap cons 1 ap ap cons 2 nil"),
            ("ap car ( 1 , 2 )", "1"),
            ("ap ap ap s mul ap add 1 6", "42"),
            ("ap ap ap c add 1 2", "3"),
            ("ap ap ap c i 1 ap i ap add 1", "2"),
            ("ap ap div 4 2", "2"),
            ("ap ap div 4 3", "1"),
            ("ap ap div 4 4", "1"),
            ("ap ap div 4 5", "0"),
            ("ap ap div 5 2", "2"),
            ("ap ap div 6 -2", "-3"),
            ("ap ap div 5 -3", "-1"),
            ("ap ap div -5 3", "-1"),
            ("ap ap div -5 -3", "1"),
        ] {
            eprintln!("--- testing: {}", tc.0);
            let env = default_env();

            let e1 = parse_string(&env, tc.0);
            let e2 = parse_string(&env, tc.1);

            eprintln!("e1: {}", e1);
            eprintln!("e2: {}", e2);

            let e1 = e1.eval(&env).demod(&env);
            let e2 = e2.eval(&env).demod(&env);

            eprintln!("e1.eval: {}", e1);
            eprintln!("e2.eval: {}", e2);

            assert_eq!(e1, e2);
        }
    }

    #[test]
    fn test_demod() {
        let env = default_env();

        for tc in [
            ("110110000111011111100001001111110100110000", "( 1 , 81740 )"),
            ("010", "0"),
            ("00", "nil"),
            ("1101000", "( 0 )"),
            ("01100001", "1"),
            ("10100001", "-1"),
        ]
        .iter()
        {
            let e1 = parse_string(&env, tc.1);
            eprintln!("e1: {}", e1);
            let e1 = e1.eval(&env).demod(&env);
            let bin = e1.modulate(&env);

            assert_eq!(tc.0, bin);
        }
    }

    #[test]
    fn test_modulate() {
        let env = default_env();

        for tc in [
            ("110110000111011111100001001111110100110000", "( 1 , 81740 )"),
            ("010", "0"),
            ("00", "nil"),
            ("1101000", "( 0 )"),
            ("01100001", "1"),
            ("10100001", "-1"),
        ]
        .iter()
        {
            let e1 = Expr::demodulate(tc.0);
            let e2 = parse_string(&env, tc.1);

            eprintln!("e1: {}", e1);
            eprintln!("e2: {}", e2);

            assert_eq!(e1, e2);
        }
    }
}

fn send_url(api_key: &str) -> String {
    format!("https://icfpc2020-api.testkontur.ru/aliens/send?apiKey={}", api_key)
}

fn send(req: &Expr, env: &Env, api_key: &str) -> Expr {
    eprintln!("sending: {}", req);
    let req = req.clone().modulate(env);
    Expr::demodulate(&request(&send_url(api_key), req))
}

#[cfg(target_os = "linux")]
fn request(url: &str, req: String) -> String {
    let client = reqwest::blocking::Client::new();
    client.post(url).body(req).send().unwrap().text().unwrap()
}

#[wasm_bindgen(module = "/define.js")]
#[cfg(target_arch = "wasm32")]
extern "C" {
    fn name() -> String;

    fn request(url: &str, req: String) -> String;
}
