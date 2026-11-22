//! A crate's source tree as the tools walk it: each file with its module path and the `cfg`s it
//! is compiled under, following `mod` declarations, `#[path]`s and inline modules.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Item, Meta, Token,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
};

/// A source file of a crate, with its module path and the `cfg`s it is compiled under.
#[derive(Clone)]
pub struct ModuleFile {
    pub module: String,
    pub path: PathBuf,
    /// The conditions of the `mod` declarations that lead to it, outermost first.
    pub cfg: Vec<String>,
}

/// One contribution to a module's items, from a file or an inline declaration in that file.
pub struct ModuleVariant {
    pub file: ModuleFile,
    /// The item indices from the file's root down to the inline module (empty for a file module).
    pub inline: Vec<usize>,
}

/// A normalized Rust cfg predicate. Both attribute extraction and stored module predicates use
/// this parser, and emitted cfg attributes quote the same tree.
#[derive(Clone)]
pub struct Cfg {
    predicate: Predicate,
    // Guarded rows in the internal catalog are keyed by their written predicates. Preserve
    // their spelling even when the emitter no longer needs the insignificant punctuation.
    written: String,
}

#[derive(Clone)]
enum Predicate {
    Bool(bool),
    All(Vec<Cfg>),
    Any(Vec<Cfg>),
    Not(Box<Cfg>),
    Atom(Meta),
}

impl Parse for Cfg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(syn::LitBool) {
            let value = input.parse::<syn::LitBool>()?.value;
            return Ok(Self {
                predicate: Predicate::Bool(value),
                written: value.to_string(),
            });
        }
        let meta: Meta = input.parse()?;
        let written = meta.to_token_stream().to_string();
        let predicate = match &meta {
            Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
                let args = list.parse_args_with(Punctuated::<Self, Token![,]>::parse_terminated)?;
                if list.path.is_ident("all") {
                    Predicate::All(args.into_iter().collect())
                } else {
                    Predicate::Any(args.into_iter().collect())
                }
            }
            Meta::List(list) if list.path.is_ident("not") => {
                Predicate::Not(Box::new(list.parse_args_with(cfg_arg)?))
            }
            _ => Predicate::Atom(meta),
        };
        Ok(Self { predicate, written })
    }
}

impl ToTokens for Cfg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match &self.predicate {
            Predicate::Bool(value) => quote!(#value),
            Predicate::All(args) => quote!(all(#(#args),*)),
            Predicate::Any(args) => quote!(any(#(#args),*)),
            Predicate::Not(arg) => quote!(not(#arg)),
            Predicate::Atom(meta) => meta.to_token_stream(),
        });
    }
}

impl Cfg {
    fn all_guards(guards: &[Self]) -> Self {
        let text = guards
            .iter()
            .map(|guard| guard.written.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Self {
            written: format!("all({text})"),
            predicate: Predicate::All(guards.to_vec()),
        }
    }

    /// Evaluate the predicate with the given cfg facts. An unknown atom matters only if neither
    /// `all` nor `any` can decide without it.
    pub fn evaluate(&self, mut atom: impl FnMut(&str) -> Option<bool>) -> Option<bool> {
        fn eval(cfg: &Cfg, atom: &mut impl FnMut(&str) -> Option<bool>) -> Option<bool> {
            match &cfg.predicate {
                Predicate::Bool(value) => Some(*value),
                Predicate::Not(arg) => eval(arg, atom).map(|value| !value),
                Predicate::Atom(meta) => atom(&atom_key(meta)),
                Predicate::All(args) | Predicate::Any(args) => {
                    let all = matches!(&cfg.predicate, Predicate::All(_));
                    let mut unknown = false;
                    for arg in args {
                        match eval(arg, atom) {
                            Some(value) if value != all => return Some(value),
                            Some(_) => {}
                            None => unknown = true,
                        }
                    }
                    (!unknown).then_some(all)
                }
            }
        }
        eval(self, &mut atom)
    }

    /// Whether the predicate is false without `test`, regardless of other build options.
    pub fn disabled_without_tests(&self) -> bool {
        self.evaluate(|atom| (atom == "test").then_some(false)) == Some(false)
    }
}

/// Parse one predicate from a cfg attribute or a `not`, with Rust's optional trailing comma.
fn cfg_arg(input: ParseStream<'_>) -> syn::Result<Cfg> {
    let cfg = input.parse()?;
    input.parse::<Option<Token![,]>>()?;
    Ok(cfg)
}

/// The predicates imposed by `attrs`, including cfgs from arbitrarily nested `cfg_attr`s.
/// A conditional attribute applies only if all the guards enclosing it hold.
pub fn cfg_predicates(attrs: &[syn::Attribute]) -> Vec<Cfg> {
    let mut conditions = Vec::new();
    for attr in attrs {
        effective_attrs(&attr.meta, &mut Vec::new(), &mut |attr, guards| {
            let Some(predicate) = attr_cfg(attr) else {
                return;
            };
            conditions.push(if guards.is_empty() {
                predicate
            } else {
                let when = Cfg::all_guards(guards);
                let skip = Cfg {
                    written: format!("not({})", when.written),
                    predicate: Predicate::Not(Box::new(when)),
                };
                Cfg {
                    written: format!("any({}, {})", skip.written, predicate.written),
                    predicate: Predicate::Any(vec![skip, predicate]),
                }
            });
        });
    }
    conditions
}

fn attr_cfg(attr: &Meta) -> Option<Cfg> {
    if let Meta::List(list) = attr
        && list.path.is_ident("cfg")
    {
        list.parse_args_with(cfg_arg).ok()
    } else if attr.path().is_ident("test") || attr.path().is_ident("bench") {
        Some(Cfg {
            predicate: Predicate::Atom(syn::parse_quote!(test)),
            written: "test".into(),
        })
    } else {
        None
    }
}

/// The attributes of an item in one resolved build, or `None` when that build does not contain it.
/// Conditional attributes are expanded in their original order; resolved `cfg` gates are gone.
/// Used when moving an item into a crate that does not share the original crate's cfgs.
pub fn configured_attrs(
    attrs: &[syn::Attribute],
    atom: impl Fn(&str) -> bool,
) -> Option<Vec<syn::Attribute>> {
    let mut active = Vec::new();
    for attr in attrs {
        let mut exists = true;
        effective_attrs(&attr.meta, &mut Vec::new(), &mut |meta, guards| {
            if guards
                .iter()
                .all(|guard| guard.evaluate(|name| Some(atom(name))) == Some(true))
            {
                if let Some(predicate) = attr_cfg(meta) {
                    exists &= predicate.evaluate(|name| Some(atom(name))) == Some(true);
                } else if guards.is_empty() {
                    active.push(attr.clone());
                } else {
                    active.push(syn::Attribute {
                        meta: meta.clone(),
                        ..attr.clone()
                    });
                }
            }
        });
        if !exists {
            return None;
        }
    }
    Some(active)
}

fn effective_attrs(meta: &Meta, guards: &mut Vec<Cfg>, f: &mut impl FnMut(&Meta, &[Cfg])) {
    if let Meta::List(list) = meta
        && list.path.is_ident("cfg_attr")
        && let Ok((guard, attrs)) = list.parse_args_with(|input: ParseStream<'_>| {
            let guard = input.parse::<Cfg>()?;
            let attrs = if input.is_empty() {
                Punctuated::new()
            } else {
                input.parse::<Token![,]>()?;
                Punctuated::<Meta, Token![,]>::parse_terminated(input)?
            };
            Ok((guard, attrs))
        })
    {
        guards.push(guard);
        for attr in &attrs {
            effective_attrs(attr, guards, f);
        }
        guards.pop();
    } else {
        f(meta, guards);
    }
}

/// The `cfg` conditions in `attrs`, including conditional `cfg` attributes, except test-only gates.
pub fn conditions(attrs: &[syn::Attribute]) -> Vec<String> {
    cfg_predicates(attrs)
        .into_iter()
        .filter(|predicate| !predicate.disabled_without_tests())
        .map(|predicate| predicate.written)
        .collect()
}

/// The normalized conditions in `attrs` as text, including test-only gates.
pub fn cfg_conditions(attrs: &[syn::Attribute]) -> Vec<String> {
    cfg_predicates(attrs)
        .into_iter()
        .map(|predicate| predicate.written)
        .collect()
}

/// Read a single variable from rustc's printed cfg set in the form predicates look it up.
pub fn cfg_atom(text: &str) -> syn::Result<String> {
    let meta = syn::parse_str::<Meta>(text)?;
    if matches!(meta, Meta::List(_)) {
        return Err(syn::Error::new_spanned(meta, "expected a cfg atom"));
    }
    Ok(atom_key(&meta))
}

fn atom_key(meta: &Meta) -> String {
    if let Meta::NameValue(entry) = meta
        && let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) = &entry.value
    {
        let path = &entry.path;
        let value = syn::LitStr::new(&value.value(), value.span());
        quote!(#path = #value).to_string()
    } else {
        meta.to_token_stream().to_string()
    }
}

/// Evaluate a written cfg predicate with the given cfg facts.
pub fn evaluate_cfg(predicate: &str, atom: impl FnMut(&str) -> Option<bool>) -> Option<bool> {
    cfg_arg.parse_str(predicate).ok()?.evaluate(atom)
}

/// Whether a node cannot be compiled with `test` off, including gates applied by `cfg_attr`.
pub fn test_only(attrs: &[syn::Attribute]) -> bool {
    cfg_predicates(attrs)
        .iter()
        .any(Cfg::disabled_without_tests)
}

/// The files of `module` read from `path`, with conventional children in `dir`. With `every_build`,
/// a module behind a `cfg` is left out with its descendants. Unless `include_tests`, test-only
/// modules are left out, including files gated by inner attributes.
pub fn walk_module_files(
    module: String,
    path: PathBuf,
    dir: PathBuf,
    every_build: bool,
    include_tests: bool,
    parse: impl Fn(&Path, &str) -> Result<syn::File, String>,
) -> Result<Vec<ModuleFile>, String> {
    Ok(
        walk_module_variants(module, path, dir, every_build, include_tests, parse)?
            .into_iter()
            .filter(|variant| variant.inline.is_empty())
            .map(|variant| variant.file)
            .collect(),
    )
}

/// Like [`walk_module_files`], but also reports inline modules with their own conditions.
pub fn walk_module_variants(
    module: String,
    path: PathBuf,
    dir: PathBuf,
    every_build: bool,
    include_tests: bool,
    parse: impl Fn(&Path, &str) -> Result<syn::File, String>,
) -> Result<Vec<ModuleVariant>, String> {
    let mut walker = Walker {
        every_build,
        include_tests,
        pending: vec![(
            ModuleFile {
                module,
                path,
                cfg: Vec::new(),
            },
            dir,
        )],
        variants: Vec::new(),
    };
    while let Some((mut file, dir)) = walker.pending.pop() {
        // A target with only conditional paths need not have a conventional file.
        if !file.path.is_file() {
            continue;
        }
        let source =
            fs::read_to_string(&file.path).map_err(|e| format!("{}: {e}", file.path.display()))?;
        let parsed = parse(&file.path, &source)?;
        if !walker.keep(&parsed.attrs) {
            continue;
        }
        file.cfg.extend(walker.conditions(&parsed.attrs));
        let here = file.path.parent().expect("a source file has a folder");
        walker.children(&file, &parsed.items, here, &dir, &mut Vec::new());
        walker.variants.push(ModuleVariant {
            file,
            inline: Vec::new(),
        });
    }
    walker
        .variants
        .sort_by(|a, b| a.file.path.cmp(&b.file.path));
    Ok(walker.variants)
}

struct Walker {
    every_build: bool,
    include_tests: bool,
    /// Each file's declaration and the directory Rust uses for conventional children.
    pending: Vec<(ModuleFile, PathBuf)>,
    variants: Vec<ModuleVariant>,
}

impl Walker {
    fn keep(&self, attrs: &[syn::Attribute]) -> bool {
        (self.include_tests || !test_only(attrs)) && !(self.every_build && gated(attrs))
    }

    fn conditions(&self, attrs: &[syn::Attribute]) -> Vec<String> {
        if self.include_tests {
            cfg_conditions(attrs)
        } else {
            conditions(attrs)
        }
    }

    /// Look through inline modules and literal `include!`s as well as regular file declarations.
    fn children(
        &mut self,
        file: &ModuleFile,
        items: &[Item],
        here: &Path,
        dir: &Path,
        inline: &mut Vec<usize>,
    ) {
        for (index, item) in items.iter().enumerate() {
            if let Item::Macro(included) = item
                && included.mac.path.is_ident("include")
                && self.keep(&included.attrs)
                && let Ok(relative) = syn::parse2::<syn::LitStr>(included.mac.tokens.clone())
            {
                let mut included_file = file.clone();
                included_file.path = file
                    .path
                    .parent()
                    .expect("a source file has a folder")
                    .join(relative.value());
                included_file.cfg.extend(self.conditions(&included.attrs));
                self.pending.push((included_file, dir.to_owned()));
            }
            let Item::Mod(declared) = item else { continue };
            if !self.keep(&declared.attrs) {
                continue;
            }
            let name = declared.ident.to_string();
            let module = format!("{}::{name}", file.module);
            let mut cfg = file.cfg.clone();
            cfg.extend(self.conditions(&declared.attrs));
            let mut paths = Vec::new();
            for attr in &declared.attrs {
                effective_attrs(&attr.meta, &mut Vec::new(), &mut |attr, guards| {
                    if let Meta::NameValue(value) = attr
                        && value.path.is_ident("path")
                        && let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(path),
                            ..
                        }) = &value.value
                    {
                        let guard = match guards {
                            [guard] => guard.written.clone(),
                            _ => Cfg::all_guards(guards).written,
                        };
                        paths.push((Some(path.value()), guard));
                    }
                });
            }
            if paths.is_empty() {
                paths.push((None, String::new()));
            } else {
                let alternatives: Vec<_> = paths.iter().map(|(_, guard)| guard.as_str()).collect();
                paths.push((None, format!("not(any({}))", alternatives.join(", "))));
            }
            for (explicit, guard) in paths {
                let mut cfg = cfg.clone();
                match evaluate_cfg(&guard, |atom| {
                    (!self.include_tests && atom == "test").then_some(false)
                }) {
                    Some(false) => continue,
                    Some(true) => {}
                    None if !guard.is_empty() => cfg.push(guard.to_owned()),
                    None => {}
                }
                if let Some((_, inner)) = &declared.content {
                    let inner_dir = dir.join(explicit.as_deref().unwrap_or(&name));
                    let child = ModuleFile {
                        module: module.clone(),
                        path: file.path.clone(),
                        cfg,
                    };
                    inline.push(index);
                    self.children(&child, inner, &inner_dir, &inner_dir, inline);
                    self.variants.push(ModuleVariant {
                        file: child,
                        inline: inline.clone(),
                    });
                    inline.pop();
                } else {
                    let (path, child_dir) = if let Some(relative) = explicit {
                        let child = here.join(relative);
                        let dir = child
                            .parent()
                            .expect("a module file has a folder")
                            .to_owned();
                        (child, dir)
                    } else if dir.join(format!("{name}.rs")).is_file() {
                        (dir.join(format!("{name}.rs")), dir.join(&name))
                    } else {
                        (dir.join(&name).join("mod.rs"), dir.join(&name))
                    };
                    self.pending.push((
                        ModuleFile {
                            module: module.clone(),
                            path,
                            cfg,
                        },
                        child_dir,
                    ));
                }
            }
        }
    }
}

/// Whether `attrs` hold a `cfg` (or `cfg_attr`): the item is compiled only in some builds.
pub fn gated(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}
