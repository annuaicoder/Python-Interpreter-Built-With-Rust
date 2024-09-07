#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use rustpython_vm::{
    builtins::{PyDictRef, PyStrRef},
    function::ArgIterable,
    identifier, AsObject, PyResult, TryFromObject, VirtualMachine,
};

pub struct ShellHelper<'vm> {
    vm: &'vm VirtualMachine,
    globals: PyDictRef,
}

fn reverse_string(s: &mut String) {
    let rev: String = s.chars().rev().collect();
    *s = rev;
}

fn split_idents_on_dot(line: &str) -> Option<(usize, Vec<String>)> {
    let mut words = vec![String::new()];
    let mut startpos = 0;
    for (i, c) in line.chars().rev().enumerate() {
        match c {
            '.' => {
                // Handle consecutive dots (invalid case)
                if i != 0 && words.last().map_or(false, |s| s.is_empty()) {
                    return None;
                }
                reverse_string(words.last_mut().unwrap());
                if words.len() == 1 {
                    startpos = line.len() - i;
                }
                words.push(String::new());
            }
            c if c.is_alphanumeric() || c == '_' => words.last_mut().unwrap().push(c),
            _ => {
                if words.len() == 1 {
                    if words.last().unwrap().is_empty() {
                        return None;
                    }
                    startpos = line.len() - i;
                }
                break;
            }
        }
    }
    if words == [String::new()] {
        return None;
    }
    reverse_string(words.last_mut().unwrap());
    words.reverse();

    Some((startpos, words))
}

impl<'vm> ShellHelper<'vm> {
    pub fn new(vm: &'vm VirtualMachine, globals: PyDictRef) -> Self {
        ShellHelper { vm, globals }
    }

    fn get_available_completions<'w>(
        &self,
        words: &'w [String],
    ) -> Option<(&'w str, impl Iterator<Item = PyResult<PyStrRef>> + 'vm)> {
        let (first, rest) = words.split_first().unwrap();

        let str_iter_method = |obj, name| {
            let iter = self.vm.call_special_method(obj, name, ())?;
            ArgIterable::<PyStrRef>::try_from_object(self.vm, iter)?.iter(self.vm)
        };

        let (word_start, iter1, iter2) = if let Some((last, parents)) = rest.split_last() {
            let mut current = self.globals.get_item_opt(first.as_str(), self.vm).ok()??;

            for attr in parents {
                let attr = self.vm.ctx.new_str(attr.as_str());
                current = current.get_attr(&attr, self.vm).ok()?;
            }

            let current_iter = str_iter_method(&current, identifier!(self.vm, __dir__)).ok()?;
            (last, current_iter, None)
        } else {
            let globals =
                str_iter_method(self.globals.as_object(), identifier!(self.vm, keys)).ok()?;
            let builtins =
                str_iter_method(self.vm.builtins.as_object(), identifier!(self.vm, __dir__)).ok()?;
            (first, globals, Some(builtins))
        };

        Some((word_start, iter1.chain(iter2.into_iter().flatten())))
    }

    fn complete_opt(&self, line: &str) -> Option<(usize, Vec<String>)> {
        let (startpos, words) = split_idents_on_dot(line)?;

        let (word_start, iter) = self.get_available_completions(&words)?;

        let all_completions = iter
            .filter(|res| {
                res.as_ref()
                    .ok()
                    .map_or(true, |s| s.as_str().starts_with(word_start))
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()?;

        let mut completions = if word_start.starts_with('_') {
            all_completions
        } else {
            let no_underscore = all_completions
                .iter()
                .filter(|&s| !s.as_str().starts_with('_'))
                .cloned()
                .collect::<Vec<_>>();

            if no_underscore.is_empty() {
                all_completions
            } else {
                no_underscore
            }
        };

        completions.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        Some((
            startpos,
            completions
                .into_iter()
                .map(|s| s.as_str().to_owned())
                .collect(),
        ))
    }
}

cfg_if::cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        use rustyline::{
            completion::Completer, highlight::Highlighter, hint::Hinter, validate::Validator, Context,
            Helper,
        };
        impl Completer for ShellHelper<'_> {
            type Candidate = String;

            fn complete(
                &self,
                line: &str,
                pos: usize,
                _ctx: &Context,
            ) -> rustyline::Result<(usize, Vec<String>)> {
                Ok(self
                    .complete_opt(&line[0..pos])
                    .unwrap_or_else(|| (pos, vec!["\t".to_owned()])))
            }
        }

        impl Hinter for ShellHelper<'_> {
            type Hint = String;
        }
        impl Highlighter for ShellHelper<'_> {}
        impl Validator for ShellHelper<'_> {}
        impl Helper for ShellHelper<'_> {}
    }
}
