//! Moves HVM Terms from runtime, and building dynamic functions.

// FIXME: `as_code` and `as_term` should just call `readback`, but before doing so, we must test
// the new readback properly to ensure it is correct

use crate::language;
use crate::runtime;
use crate::runtime::{Heap, Program, Ptr, Tag};
use std::collections::{hash_map, HashMap, HashSet};

/// Reads back a term from Runtime's memory
pub fn as_code(heap: &Heap, prog: &Program, host: u64) -> String {
  format!("{}", as_term(heap, prog, host))
}

/// Reads back a term from Runtime's memory
pub fn as_term(heap: &Heap, prog: &Program, host: u64) -> Box<language::syntax::Term> {
  struct CtxName<'a> {
    heap: &'a Heap,
    prog: &'a Program,
    names: &'a mut HashMap<Ptr, String>,
    seen: &'a mut HashSet<Ptr>,
  }

  fn gen_var_names(heap: &Heap, prog: &Program, ctx: &mut CtxName, term: Ptr, depth: u32) {
    if ctx.seen.contains(&term) {
      return;
    };

    ctx.seen.insert(term);

    match runtime::get_tag(term) {
      Tag::LAM => {
        let param = ctx.heap.load_arg(term, 0);
        let body = ctx.heap.load_arg(term, 1);
        if runtime::get_tag(param) != Tag::ERA {
          let var = runtime::Var(runtime::get_loc(term, 0));
          ctx.names.insert(var, format!("x{}", ctx.names.len()));
        };
        gen_var_names(heap, prog, ctx, body, depth + 1);
      }
      Tag::APP => {
        let lam = ctx.heap.load_arg(term, 0);
        let arg = ctx.heap.load_arg(term, 1);
        gen_var_names(heap, prog, ctx, lam, depth + 1);
        gen_var_names(heap, prog, ctx, arg, depth + 1);
      }
      Tag::SUP => {
        let arg0 = ctx.heap.load_arg(term, 0);
        let arg1 = ctx.heap.load_arg(term, 1);
        gen_var_names(heap, prog, ctx, arg0, depth + 1);
        gen_var_names(heap, prog, ctx, arg1, depth + 1);
      }
      Tag::DP0 => {
        let arg = ctx.heap.load_arg(term, 2);
        gen_var_names(heap, prog, ctx, arg, depth + 1);
      }
      Tag::DP1 => {
        let arg = ctx.heap.load_arg(term, 2);
        gen_var_names(heap, prog, ctx, arg, depth + 1);
      }
      Tag::OP2 => {
        let arg0 = ctx.heap.load_arg(term, 0);
        let arg1 = ctx.heap.load_arg(term, 1);
        gen_var_names(heap, prog, ctx, arg0, depth + 1);
        gen_var_names(heap, prog, ctx, arg1, depth + 1);
      }
      Tag::U60 => {}
      Tag::F60 => {}
      Tag::CTR | Tag::FUN => {
        let arity = runtime::arity_of(&ctx.prog.aris, term);
        for i in 0..arity {
          let arg = ctx.heap.load_arg(term, i);
          gen_var_names(heap, prog, ctx, arg, depth + 1);
        }
      }
      _ => {}
    }
  }

  #[allow(dead_code)]
  struct CtxGo<'a> {
    heap: &'a Heap,
    prog: &'a Program,
    names: &'a HashMap<Ptr, String>,
    seen: &'a HashSet<Ptr>,
  }

  struct Stacks {
    stacks: HashMap<Ptr, Vec<bool>>,
  }

  impl Stacks {
    fn new() -> Self {
      Self { stacks: HashMap::new() }
    }
    fn get(&self, col: Ptr) -> Option<&Vec<bool>> {
      self.stacks.get(&col)
    }
    fn pop(&mut self, col: Ptr) -> bool {
      let stack = self.stacks.entry(col).or_insert_with(Vec::new);
      stack.pop().unwrap_or(false)
    }
    fn push(&mut self, col: Ptr, val: bool) {
      let stack = self.stacks.entry(col).or_insert_with(Vec::new);
      stack.push(val);
    }
  }

  fn readback(
    heap: &Heap,
    prog: &Program,
    ctx: &mut CtxGo,
    stacks: &mut Stacks,
    term: Ptr,
    depth: u32,
  ) -> Box<language::syntax::Term> {
    match runtime::get_tag(term) {
      Tag::LAM => {
        let body = ctx.heap.load_arg(term, 1);
        let body = readback(heap, prog, ctx, stacks, body, depth + 1);
        let bind = ctx.heap.load_arg(term, 0);
        let name = if runtime::get_tag(bind) == Tag::ERA {
          "*".to_string()
        } else {
          let var = runtime::Var(runtime::get_loc(term, 0));
          ctx.names.get(&var).map(|s| s.clone()).unwrap_or("?".to_string())
        };
        Box::new(language::syntax::Term::Lam { name, body })
      }
      Tag::APP => {
        let func = ctx.heap.load_arg(term, 0);
        let argm = ctx.heap.load_arg(term, 1);
        let func = readback(heap, prog, ctx, stacks, func, depth + 1);
        let argm = readback(heap, prog, ctx, stacks, argm, depth + 1);
        Box::new(language::syntax::Term::App { func, argm })
      }
      Tag::SUP => {
        let col = runtime::get_ext(term);
        let empty = &vec![];
        let stack = stacks.get(col).unwrap_or(empty);
        if let Some(val) = stack.last() {
          let arg_idx = *val as u64;
          let val = ctx.heap.load_arg(term, arg_idx);
          let old = stacks.pop(col);
          let got = readback(heap, prog, ctx, stacks, val, depth + 1);
          stacks.push(col, old);
          got
        } else {
          let val0 = ctx.heap.load_arg(term, 0);
          let val1 = ctx.heap.load_arg(term, 1);
          let val0 = readback(heap, prog, ctx, stacks, val0, depth + 1);
          let val1 = readback(heap, prog, ctx, stacks, val1, depth + 1);
          Box::new(language::syntax::Term::Sup { val0, val1 })
        }
      }
      Tag::DP0 => {
        let col = runtime::get_ext(term);
        let val = ctx.heap.load_arg(term, 2);
        stacks.push(col, false);
        let result = readback(heap, prog, ctx, stacks, val, depth + 1);
        stacks.pop(col);
        result
      }
      Tag::DP1 => {
        let col = runtime::get_ext(term);
        let val = ctx.heap.load_arg(term, 2);
        stacks.push(col, true);
        let result = readback(heap, prog, ctx, stacks, val, depth + 1);
        stacks.pop(col);
        result
      }
      Tag::OP2 => {
        let oper = runtime::get_oper(term);
        let val0 = ctx.heap.load_arg(term, 0);
        let val1 = ctx.heap.load_arg(term, 1);
        let val0 = readback(heap, prog, ctx, stacks, val0, depth + 1);
        let val1 = readback(heap, prog, ctx, stacks, val1, depth + 1);
        Box::new(language::syntax::Term::Op2 { oper, val0, val1 })
      }
      Tag::U60 => {
        let numb = runtime::get_num(term);
        Box::new(language::syntax::Term::U6O { numb })
      }
      Tag::F60 => {
        let numb = runtime::get_num(term);
        Box::new(language::syntax::Term::F6O { numb })
      }
      Tag::CTR | Tag::FUN => {
        let func = runtime::get_ext(term);
        let arit = runtime::arity_of(&ctx.prog.aris, term);
        let mut args = vec![];
        for i in 0..arit {
          let arg = ctx.heap.load_arg(term, i);
          args.push(readback(heap, prog, ctx, stacks, arg, depth + 1));
        }
        let name =
          ctx.prog.nams.get(&func).map(String::to_string).unwrap_or_else(|| format!("${}", func));
        Box::new(language::syntax::Term::Ctr { name, args })
      }
      Tag::VAR => {
        let name = ctx
          .names
          .get(&term)
          .map(String::to_string)
          .unwrap_or_else(|| format!("^{}", runtime::get_loc(term, 0)));
        Box::new(language::syntax::Term::Var { name }) // ............... /\ why this sounds so threatening?
      }
      Tag::ARG => Box::new(language::syntax::Term::Var { name: "<arg>".to_string() }),
      Tag::ERA => Box::new(language::syntax::Term::Var { name: "<era>".to_string() }),
      _ => Box::new(language::syntax::Term::Var {
        name: format!("<unknown_tag_{}>", runtime::get_tag(term)),
      }),
    }
  }

  let term = heap.load_ptr(host);

  let mut names = HashMap::<Ptr, String>::new();
  let mut seen = HashSet::<Ptr>::new();

  let ctx = &mut CtxName { heap, prog, names: &mut names, seen: &mut seen };
  gen_var_names(heap, prog, ctx, term, 0);

  let ctx = &mut CtxGo { heap, prog, names: &names, seen: &seen };
  let mut stacks = Stacks::new();
  readback(heap, prog, ctx, &mut stacks, term, 0)
}

impl Heap {
  // Reads a term linearly, i.e., preserving dups
  pub fn as_linear_term(&self, prog: &Program, host: u64) -> Box<language::syntax::Term> {
    enum StackItem {
      Term(Ptr),
      Resolver(Ptr),
    }

    fn ctr_name(prog: &Program, id: u64) -> String {
      if let Some(name) = prog.nams.get(&id) {
        return name.clone();
      } else {
        return format!("${}", id);
      }
    }

    fn dups(
      heap: &Heap,
      prog: &Program,
      term: Ptr,
      names: &mut HashMap<u64, String>,
    ) -> language::syntax::Term {
      let mut lets: HashMap<u64, u64> = HashMap::new();
      let mut kinds: HashMap<u64, u64> = HashMap::new();
      let mut stack = vec![term];
      while !stack.is_empty() {
        let term = stack.pop().unwrap();
        match runtime::get_tag(term) {
          Tag::LAM => {
            names.insert(runtime::get_loc(term, 0), format!("{}", names.len()));
            stack.push(heap.load_arg(term, 1));
          }
          Tag::APP => {
            stack.push(heap.load_arg(term, 1));
            stack.push(heap.load_arg(term, 0));
          }
          Tag::SUP => {
            stack.push(heap.load_arg(term, 1));
            stack.push(heap.load_arg(term, 0));
          }
          Tag::DP0 => {
            if let hash_map::Entry::Vacant(e) = lets.entry(runtime::get_loc(term, 0)) {
              names.insert(runtime::get_loc(term, 0), format!("{}", names.len()));
              kinds.insert(runtime::get_loc(term, 0), runtime::get_ext(term));
              e.insert(runtime::get_loc(term, 0));
              stack.push(heap.load_arg(term, 2));
            }
          }
          Tag::DP1 => {
            if let hash_map::Entry::Vacant(e) = lets.entry(runtime::get_loc(term, 0)) {
              names.insert(runtime::get_loc(term, 0), format!("{}", names.len()));
              kinds.insert(runtime::get_loc(term, 0), runtime::get_ext(term));
              e.insert(runtime::get_loc(term, 0));
              stack.push(heap.load_arg(term, 2));
            }
          }
          Tag::OP2 => {
            stack.push(heap.load_arg(term, 1));
            stack.push(heap.load_arg(term, 0));
          }
          Tag::CTR | Tag::FUN => {
            let arity = runtime::arity_of(&prog.aris, term);
            for i in (0..arity).rev() {
              stack.push(heap.load_arg(term, i));
            }
          }
          _ => {}
        }
      }

      let cont = expr(heap, prog, term, &names);
      if lets.is_empty() {
        cont
      } else {
        let mut output = language::syntax::Term::Var { name: "?".to_string() };
        for (i, (_key, pos)) in lets.iter().enumerate() {
          // todo: reverse
          let what = String::from("?h");
          let name = names.get(&pos).unwrap_or(&what);
          let nam0 = if heap.load_ptr(pos + 0) == runtime::Era() {
            String::from("*")
          } else {
            format!("a{}", name)
          };
          let nam1 = if heap.load_ptr(pos + 1) == runtime::Era() {
            String::from("*")
          } else {
            format!("b{}", name)
          };
          let expr = expr(heap, prog, heap.load_ptr(pos + 2), &names);
          if i == 0 {
            output = language::syntax::Term::Dup {
              nam0,
              nam1,
              expr: Box::new(expr),
              body: Box::new(cont.clone()),
            };
          } else {
            output = language::syntax::Term::Dup {
              nam0,
              nam1,
              expr: Box::new(expr),
              body: Box::new(output),
            };
          }
        }
        output
      }
    }

    fn expr(
      heap: &Heap,
      prog: &Program,
      term: Ptr,
      names: &HashMap<u64, String>,
    ) -> language::syntax::Term {
      let mut stack = vec![StackItem::Term(term)];
      let mut output: Vec<language::syntax::Term> = vec![];
      while !stack.is_empty() {
        let item = stack.pop().unwrap();
        match item {
          StackItem::Resolver(term) => match runtime::get_tag(term) {
            Tag::CTR => {
              let func = runtime::get_ext(term);
              let arit = runtime::arity_of(&prog.aris, term);
              let mut args = vec![];
              for _ in 0..arit {
                args.push(Box::new(output.pop().unwrap()));
              }
              let name = ctr_name(prog, func);
              output.push(language::syntax::Term::Ctr { name, args });
            }
            Tag::FUN => {
              let func = runtime::get_ext(term);
              let arit = runtime::arity_of(&prog.aris, term);
              let mut args = vec![];
              for _ in 0..arit {
                args.push(Box::new(output.pop().unwrap()));
              }
              let name = ctr_name(prog, func);
              output.push(language::syntax::Term::Ctr { name, args });
            }
            Tag::LAM => {
              let name =
                format!("x{}", names.get(&runtime::get_loc(term, 0)).unwrap_or(&String::from("?")));
              let body = Box::new(output.pop().unwrap());
              output.push(language::syntax::Term::Lam { name, body });
            }
            Tag::APP => {
              let argm = Box::new(output.pop().unwrap());
              let func = Box::new(output.pop().unwrap());
              output.push(language::syntax::Term::App { func, argm });
            }
            Tag::OP2 => {
              let oper = runtime::get_oper(term);
              let val1 = Box::new(output.pop().unwrap());
              let val0 = Box::new(output.pop().unwrap());
              output.push(language::syntax::Term::Op2 { oper, val0, val1 })
            }
            _ => panic!("Term not valid in readback"),
          },
          StackItem::Term(term) => match runtime::get_tag(term) {
            Tag::DP0 => {
              let name = format!(
                "a{}",
                names.get(&runtime::get_loc(term, 0)).unwrap_or(&String::from("?a"))
              );
              output.push(language::syntax::Term::Var { name });
            }
            Tag::DP1 => {
              let name = format!(
                "b{}",
                names.get(&runtime::get_loc(term, 0)).unwrap_or(&String::from("?b"))
              );
              output.push(language::syntax::Term::Var { name });
            }
            Tag::VAR => {
              let name = format!(
                "x{}",
                names.get(&runtime::get_loc(term, 0)).unwrap_or(&String::from("?x"))
              );
              output.push(language::syntax::Term::Var { name });
            }
            Tag::LAM => {
              stack.push(StackItem::Resolver(term));
              stack.push(StackItem::Term(heap.load_arg(term, 1)));
            }
            Tag::APP => {
              stack.push(StackItem::Resolver(term));
              stack.push(StackItem::Term(heap.load_arg(term, 1)));
              stack.push(StackItem::Term(heap.load_arg(term, 0)));
            }
            Tag::SUP => {}
            Tag::OP2 => {
              stack.push(StackItem::Resolver(term));
              stack.push(StackItem::Term(heap.load_arg(term, 1)));
              stack.push(StackItem::Term(heap.load_arg(term, 0)));
            }
            Tag::U60 => {
              let numb = runtime::get_num(term);
              output.push(language::syntax::Term::U6O { numb });
            }
            Tag::F60 => {
              let numb = runtime::get_num(term);
              output.push(language::syntax::Term::F6O { numb });
            }
            Tag::CTR => {
              let arit = runtime::arity_of(&prog.aris, term);
              stack.push(StackItem::Resolver(term));
              for i in 0..arit {
                stack.push(StackItem::Term(heap.load_arg(term, i)));
              }
            }
            Tag::FUN => {
              let arit = runtime::arity_of(&prog.aris, term);
              stack.push(StackItem::Resolver(term));
              for i in 0..arit {
                stack.push(StackItem::Term(heap.load_arg(term, i)));
              }
            }
            // Tag::ERA => {}
            _ => {}
          },
        }
      }
      output.pop().unwrap()
    }

    let mut names: HashMap<u64, String> = HashMap::new();
    Box::new(dups(self, prog, self.load_ptr(host), &mut names))
  }

  /// Reads back a term from Runtime's memory
  pub fn as_linear_code(&self, prog: &Program, host: u64) -> String {
    format!("{}", self.as_linear_term(prog, host))
  }

  // This reads a term in the `(String.cons ... String.nil)` shape directly into a string.
  pub fn as_string(&self, prog: &Program, tids: &[usize], host: u64) -> Option<String> {
    let mut host = host;
    let mut text = String::new();
    self.reduce(prog, tids, host, true, false);
    loop {
      let term = self.load_ptr(host);
      if runtime::get_tag(term) == Tag::CTR {
        let fid = runtime::get_ext(term);
        if fid == runtime::STRING_NIL {
          break;
        }
        if fid == runtime::STRING_CONS {
          let chr = self.load_ptr(runtime::get_loc(term, 0));
          if runtime::get_tag(chr) == Tag::U60 {
            text.push(std::char::from_u32(runtime::get_num(chr) as u32).unwrap_or('?'));
            host = runtime::get_loc(term, 1);
            continue;
          } else {
            return None;
          }
        }
        return None;
      } else {
        return None;
      }
    }
    Some(text)
  }
}
