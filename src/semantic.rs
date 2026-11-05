//! Bounded, dependency-free scope graph built from the retained CST.
//!
//! The graph exists so a rule can ask what an identifier refers to instead of
//! matching text. It models exactly the constructs listed in
//! `docs/semantic-model.md`; every other name-bearing construct is recorded as
//! *unsupported* by kind, which clears [`ScopeGraph::is_complete`]. A semantic
//! rule must obtain its graph through [`ScopeGraph::reportable`], so an
//! incomplete model can never produce a finding.

use std::{collections::BTreeMap, ops::Range};

use serde::Serialize;
use tree_sitter::Node;

use crate::{
    source::{Failure, Source},
    syntax::Document,
};

/// Version of the `--dump-scopes` document. Unstable diagnostics surface.
pub const SCHEMA_VERSION: u32 = 1;

/// Resource bounds. [`Document::parse`] already limits the CST to depth 512 and
/// 1,000,000 fingerprint events; these caps bound the additional scope-graph
/// allocations for the same input, and an overrun fails the file closed.
pub const MAX_SCOPES: usize = 10_000;
pub const MAX_BINDINGS: usize = 100_000;
pub const MAX_REFERENCES: usize = 250_000;
pub const MAX_SKIPPED_KINDS: usize = 64;

/// Key that absorbs skipped kinds beyond [`MAX_SKIPPED_KINDS`], so the map stays
/// bounded without ever hiding that unsupported constructs exist.
const OTHER_KIND: &str = "other";

/// An owning region. `parent` is the enclosing scope index, absent only for the
/// file scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScopeKind {
    File,
    Module,
    Class,
    Function,
    Block,
    For,
    If,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<usize>,
}

/// What a declaration introduces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingKind {
    Variable,
    Constant,
    Parameter,
    Function,
    Class,
    Enum,
    Struct,
    Interface,
    /// The pinned grammar produces `type_definition` for both `Name := class`
    /// and an immutable value binding, so this variant stays reserved until a
    /// CST shape can distinguish a real alias.
    #[allow(dead_code)]
    TypeAlias,
    ModuleImport,
    ForBinding,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub name: String,
    pub kind: BindingKind,
    /// Index into [`ScopeGraph::scopes`]; the scope that owns the binding.
    pub scope: usize,
    /// Source byte range of the declaration name, BOM included.
    pub range: Range<usize>,
    pub mutable: bool,
    /// The declaration is reachable from outside this file, by an explicit
    /// `public`/`protected` specifier or because it is a top-level type name.
    /// Cross-file reachability is out of scope, so this flag exists to stop a
    /// rule from calling an exported name unused; it is not a visibility model.
    pub exported: bool,
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub name: String,
    /// Source byte range of the referencing identifier, BOM included.
    pub range: Range<usize>,
    /// Index into [`ScopeGraph::scopes`] of the scope the reference appears in.
    pub scope: usize,
    /// Member lookup starts here instead of [`Self::scope`]: the nearest
    /// enclosing class scope for `self.Field`. Ordinary references leave it
    /// `None`, so they walk their own scope chain. Member syntax must not see a
    /// same-named local, so this is tracked per reference rather than folded
    /// into `scope`, which stays the lexically enclosing scope.
    pub receiver: Option<usize>,
}

/// Scopes in creation order, bindings and references in walk order. Nothing is
/// read back through a hashed container, so the same source always produces the
/// same indices.
#[derive(Debug)]
pub struct ScopeGraph {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    /// Bindings owned by each scope, in creation order; resolution scans only
    /// these, never a map.
    by_scope: Vec<Vec<usize>>,
    references: Vec<Reference>,
    skipped: BTreeMap<&'static str, usize>,
}

impl ScopeGraph {
    /// Build the graph for a document that already passed every coverage guard
    /// of [`Document::parse`]. Nothing here re-parses or repairs the tree.
    pub fn build(source: &Source, document: &Document) -> Result<Self, Failure> {
        let body_start = document.body_start();
        let mut builder = Builder {
            text: &source.text().as_bytes()[body_start..],
            body_start,
            scopes: vec![Scope {
                kind: ScopeKind::File,
                parent: None,
            }],
            by_scope: vec![Vec::new()],
            bindings: Vec::new(),
            references: Vec::new(),
            skipped: BTreeMap::new(),
        };
        builder.walk(document.root(), 0)?;
        Ok(Self {
            scopes: builder.scopes,
            bindings: builder.bindings,
            by_scope: builder.by_scope,
            references: builder.references,
            skipped: builder.skipped,
        })
    }

    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    /// References appearing in one scope, in walk order. References are also
    /// available flat through [`Self::references`]; this view answers the same
    /// question from the scope side without a hashed index.
    #[allow(dead_code)]
    pub fn references_in(&self, scope: usize) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(move |reference| reference.scope == scope)
    }

    /// Bindings owned by one scope, in declaration order.
    #[allow(dead_code)]
    pub fn bindings_in(&self, scope: usize) -> impl Iterator<Item = &Binding> {
        self.by_scope[scope]
            .iter()
            .map(|index| &self.bindings[*index])
    }

    /// Constructs the walker could not classify, counted by CST node kind.
    pub fn skipped(&self) -> &BTreeMap<&'static str, usize> {
        &self.skipped
    }

    /// `false` when any name-bearing construct was left unmodelled. Every
    /// recorded skip invalidates name resolution, so a new grammar shape fails
    /// closed instead of looking resolved.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }

    /// The only graph a semantic rule may report from. `None` is a documented
    /// no-op, not a warning: the model cannot prove the names it would blame.
    pub fn reportable(&self) -> Option<&Self> {
        self.is_complete().then_some(self)
    }

    /// Bindings with zero resolved references in this graph, in declaration
    /// order. Call through [`Self::reportable`]: called on an incomplete graph
    /// this returns nothing, because a name the model could not see used must
    /// never be called unused.
    ///
    /// Candidates are the constructs whose absence of a reference is provable
    /// from one file:
    ///
    /// - a `variable` or `constant` local declared in a function body;
    /// - that function's `parameter`, only when `parameters` is set, because
    ///   an unused parameter is often dictated by a callback signature.
    ///
    /// Everything else is excluded by construction rather than by a list of
    /// exceptions: an exported binding needs no intra-file reference, a
    /// `moduleImport` may be re-exported or untraceable, and a `for`/`if`
    /// binding exists to test or iterate the value it binds, which is not
    /// evidence that the declaration is dead. A `_`-prefixed name is the
    /// conventional "deliberately unused" marker.
    ///
    /// A resolved reference *writes* as well as reads: this proves no reference
    /// at all, never "no read", so an assigned-but-unread local stays silent.
    pub fn unused(&self, parameters: bool) -> Vec<&Binding> {
        if !self.is_complete() {
            return Vec::new();
        }
        let mut used = vec![false; self.bindings.len()];
        for reference in &self.references {
            if let Some(index) = self.resolve_position(reference) {
                used[index] = true;
            }
        }
        self.bindings
            .iter()
            .enumerate()
            .filter(|(index, binding)| {
                !used[*index] && self.is_unused_candidate(binding, parameters)
            })
            .map(|(_, binding)| binding)
            .collect()
    }

    fn is_unused_candidate(&self, binding: &Binding, parameters: bool) -> bool {
        if binding.exported || binding.name.starts_with('_') {
            return false;
        }
        match binding.kind {
            BindingKind::Parameter => parameters,
            BindingKind::Variable | BindingKind::Constant => {
                // Only a body local: a file-scope binding, a class field and a
                // module-level value are all reachable from elsewhere. A
                // `for`/`if` condition binding owns a non-block scope and is
                // never a candidate, for the reason in `Self::unused`.
                matches!(
                    self.scopes[binding.scope].kind,
                    ScopeKind::Block | ScopeKind::Function
                ) && self.has_ancestor(binding.scope, ScopeKind::Function)
            }
            _ => false,
        }
    }

    fn has_ancestor(&self, scope: usize, kind: ScopeKind) -> bool {
        let mut current = Some(scope);
        while let Some(index) = current {
            if self.scopes[index].kind == kind {
                return true;
            }
            current = self.scopes[index].parent;
        }
        false
    }

    /// Resolve a reference by walking its scope chain outward.
    ///
    /// Inside one scope the *last* declared binding with that name wins, and a
    /// binding only counts once its declaration starts before the reference, so
    /// an inner block shadows from its declaration onward while a name used
    /// before a redeclaration keeps the outer binding. An unknown name returns
    /// `None`; it is never matched to a guess.
    #[allow(dead_code)]
    pub fn resolve(&self, reference: &Reference) -> Option<&Binding> {
        self.resolve_position(reference)
            .map(|index| &self.bindings[index])
    }
    /// Resolution as a binding index, which is what a caller comparing
    /// bindings needs; scanning the owning scope's ordered list keeps this
    /// independent of any hashed container.
    fn resolve_position(&self, reference: &Reference) -> Option<usize> {
        let mut scope = Some(reference.receiver.unwrap_or(reference.scope));
        while let Some(index) = scope {
            for binding in self.by_scope[index].iter().rev() {
                let candidate = &self.bindings[*binding];
                // A member is looked up in the type, not forward through the
                // source, so the declaration-order rule applies only to ordinary
                // names. Fields may be declared after the method that reads them.
                let visible = if reference.receiver.is_some() {
                    true
                } else {
                    candidate.range.start < reference.range.start
                };
                if candidate.name == reference.name && visible {
                    return Some(*binding);
                }
            }
            scope = self.scopes[index].parent;
        }
        None
    }

    /// Index into [`ScopeGraph::bindings`] of what the reference resolves to, so
    /// `--dump-scopes` points at a binding instead of duplicating it.
    pub fn resolved_index(&self, reference: &Reference) -> Option<usize> {
        self.resolve_position(reference)
    }

    /// Unstable per-file view used by `--dump-scopes`.
    pub fn dump(&self, path: &str) -> FileDump {
        FileDump {
            path: path.to_string(),
            scopes: self
                .scopes()
                .iter()
                .map(|scope| ScopeDump {
                    kind: scope.kind,
                    parent: scope.parent,
                })
                .collect(),
            bindings: self
                .bindings()
                .iter()
                .map(|binding| BindingDump {
                    name: binding.name.clone(),
                    kind: binding.kind,
                    scope: binding.scope,
                    range: RangeDump::new(&binding.range),
                    mutable: binding.mutable,
                    exported: binding.exported,
                })
                .collect(),
            references: self
                .references()
                .iter()
                .map(|reference| ReferenceDump {
                    name: reference.name.clone(),
                    range: RangeDump::new(&reference.range),
                    scope: reference.scope,
                    resolved: self.resolved_index(reference),
                })
                .collect(),
            skipped: self
                .skipped()
                .iter()
                .map(|(kind, count)| ((*kind).to_string(), *count))
                .collect(),
            is_complete: self.is_complete(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeDump {
    start: usize,
    end: usize,
}

impl RangeDump {
    fn new(range: &Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDump {
    kind: ScopeKind,
    parent: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingDump {
    name: String,
    kind: BindingKind,
    scope: usize,
    range: RangeDump,
    mutable: bool,
    exported: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceDump {
    name: String,
    range: RangeDump,
    scope: usize,
    /// Index into `bindings`, or `null` when the name is unresolved.
    resolved: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDump {
    path: String,
    scopes: Vec<ScopeDump>,
    bindings: Vec<BindingDump>,
    references: Vec<ReferenceDump>,
    skipped: BTreeMap<String, usize>,
    is_complete: bool,
}

/// The `--dump-scopes` document: one entry per analysed source, in input order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDump {
    schema_version: u32,
    files: Vec<FileDump>,
}

impl GraphDump {
    pub fn new(files: Vec<FileDump>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            files,
        }
    }

    pub fn to_json(&self) -> Result<String, String> {
        let mut text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        text.push('\n');
        Ok(text)
    }
}

struct Builder<'a> {
    /// Source body bytes: the same slice the CST byte offsets are relative to.
    text: &'a [u8],
    body_start: usize,
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    by_scope: Vec<Vec<usize>>,
    references: Vec<Reference>,
    skipped: BTreeMap<&'static str, usize>,
}

impl Builder<'_> {
    fn start(&self, node: Node<'_>) -> usize {
        node.start_byte() + self.body_start
    }

    fn range(&self, node: Node<'_>) -> Range<usize> {
        self.start(node)..node.end_byte() + self.body_start
    }

    fn text_of(&self, node: Node<'_>) -> Option<&str> {
        node.utf8_text(self.text).ok()
    }

    fn open(&mut self, kind: ScopeKind, parent: usize, at: Node<'_>) -> Result<usize, Failure> {
        if self.scopes.len() >= MAX_SCOPES {
            return Err(Failure::new(
                self.start(at),
                "scope graph scope limit exceeded",
            ));
        }
        self.scopes.push(Scope {
            kind,
            parent: Some(parent),
        });
        self.by_scope.push(Vec::new());
        Ok(self.scopes.len() - 1)
    }

    fn bind(
        &mut self,
        scope: usize,
        node: Node<'_>,
        kind: BindingKind,
        mutable: bool,
    ) -> Result<(), Failure> {
        let Some(name) = self.text_of(node).map(str::to_owned) else {
            self.skip("identifier");
            return Ok(());
        };
        if self.bindings.len() >= MAX_BINDINGS {
            return Err(Failure::new(
                self.start(node),
                "scope graph binding limit exceeded",
            ));
        }
        let exported = self.is_exported(node, kind, scope);
        self.bindings.push(Binding {
            name,
            kind,
            scope,
            range: self.range(node),
            mutable,
            exported,
        });
        self.by_scope[scope].push(self.bindings.len() - 1);
        Ok(())
    }

    /// Whether a declaration is reachable from outside this file. Cross-file
    /// reachability is out of scope for this wave, so an explicit
    /// `public`/`protected` specifier and any top-level type name both count as
    /// exported: intra-file usage proves nothing about them.
    fn is_exported(&self, name: Node<'_>, kind: BindingKind, scope: usize) -> bool {
        if matches!(
            kind,
            BindingKind::Class | BindingKind::Struct | BindingKind::Interface | BindingKind::Enum
        ) && self.scopes[scope].kind == ScopeKind::File
        {
            return true;
        }
        let Some(declaration) = name.parent() else {
            return false;
        };
        // `Name := class<public>:` puts the specifier on the value, not on the
        // `type_definition` that names the type.
        let value_of_type = (declaration.kind() == "type_definition")
            .then(|| declaration.child_by_field_name("value"))
            .flatten();
        std::iter::once(declaration)
            .chain(value_of_type)
            .any(|owner| self.has_exported_specifier(owner))
    }

    fn has_exported_specifier(&self, node: Node<'_>) -> bool {
        ["specifiers", "var_specifiers", "name_specifiers"]
            .into_iter()
            .filter_map(|field| node.child_by_field_name(field))
            .any(|list| {
                let mut cursor = list.walk();
                list.named_children(&mut cursor).any(|specifier| {
                    specifier
                        .child_by_field_name("name")
                        .and_then(|name| self.text_of(name))
                        .is_some_and(|name| matches!(name, "public" | "protected"))
                })
            })
    }

    /// Declare a node only when it really is a plain identifier. Anything else
    /// (a scoped name, an operator function name, …) is unsupported instead of
    /// being silently reclassified.
    fn declare(
        &mut self,
        scope: usize,
        node: Node<'_>,
        kind: BindingKind,
        mutable: bool,
    ) -> Result<(), Failure> {
        if node.kind() != "identifier" {
            self.skip(node.kind());
            return Ok(());
        }
        self.bind(scope, node, kind, mutable)
    }

    fn reference(&mut self, scope: usize, node: Node<'_>) -> Result<(), Failure> {
        self.reference_from(scope, None, node)
    }

    /// Records a reference whose lookup starts at `receiver` rather than at the
    /// lexically enclosing `scope` (used for `self.Field`).
    fn reference_from(
        &mut self,
        scope: usize,
        receiver: Option<usize>,
        node: Node<'_>,
    ) -> Result<(), Failure> {
        let Some(name) = self.text_of(node).map(str::to_owned) else {
            self.skip("identifier");
            return Ok(());
        };
        if self.references.len() >= MAX_REFERENCES {
            return Err(Failure::new(
                self.start(node),
                "scope graph reference limit exceeded",
            ));
        }
        self.references.push(Reference {
            name,
            range: self.range(node),
            scope,
            receiver,
        });
        Ok(())
    }

    /// Nearest enclosing class scope, for `self.Member`. `None` when `self` is
    /// used outside a class: that is not something to guess about.
    fn enclosing_class(&self, mut scope: usize) -> Option<usize> {
        loop {
            if self.scopes[scope].kind == ScopeKind::Class {
                return Some(scope);
            }
            scope = self.scopes[scope].parent?;
        }
    }

    fn skip(&mut self, kind: &'static str) {
        if let Some(count) = self.skipped.get_mut(kind) {
            *count += 1;
            return;
        }
        if kind != OTHER_KIND && self.skipped.len() + 1 >= MAX_SKIPPED_KINDS {
            *self.skipped.entry(OTHER_KIND).or_insert(0) += 1;
            return;
        }
        self.skipped.insert(kind, 1);
    }

    fn walk_children(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, scope)?;
        }
        Ok(())
    }

    fn walk(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        match node.kind() {
            // Scope owners. Each opens one region shared by its body/members;
            // no existing coverage guard changes.
            "block_expression" | "brace_block" | "colon_block" | "indented_block" | "dot_block" => {
                let inner = self.open(ScopeKind::Block, scope, node)?;
                self.walk_children(node, inner)?;
            }
            "class_definition" | "struct_definition" | "interface_definition" => {
                let inner = self.open(ScopeKind::Class, scope, node)?;
                self.walk_type_body(node, scope, inner)?;
            }
            "module_definition" => {
                let inner = self.open(ScopeKind::Module, scope, node)?;
                self.walk_children(node, inner)?;
            }
            "enum_definition" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "enum_variant" {
                        // Variants are enum members; the working set does not
                        // bind them, so the graph stays incomplete rather than
                        // pretending the names do not exist.
                        self.skip("enum_variant");
                    } else {
                        self.walk(child, scope)?;
                    }
                }
            }
            "function_definition" | "extension_function_definition" => {
                let inner = self.open(ScopeKind::Function, scope, node)?;
                if let Some(name) = node.child_by_field_name("name") {
                    self.declare(scope, name, BindingKind::Function, false)?;
                }
                // Signature parts cannot see the parameters.
                for field in ["specifiers", "effect_specifiers", "return_type"] {
                    if let Some(child) = node.child_by_field_name(field) {
                        self.walk(child, scope)?;
                    }
                }
                // Parameters and body live in the function scope.
                for field in ["receiver", "parameters", "body"] {
                    if let Some(child) = node.child_by_field_name(field) {
                        self.walk(child, inner)?;
                    }
                }
            }
            "type_definition" => self.walk_type_definition(node, scope)?,
            "constant_declaration" | "var_declaration" | "field_declaration" => {
                let mutable = node.kind() == "var_declaration";
                let kind = if mutable {
                    BindingKind::Variable
                } else {
                    BindingKind::Constant
                };
                if let Some(name) = node.child_by_field_name("name") {
                    self.declare(scope, name, kind, mutable)?;
                }
                for field in [
                    "var_specifiers",
                    "specifiers",
                    "name_specifiers",
                    "type",
                    "value",
                    "default",
                ] {
                    if let Some(child) = node.child_by_field_name(field) {
                        self.walk(child, scope)?;
                    }
                }
            }
            "using_declaration" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    match child.kind() {
                        "local_module_path" => self.walk_local_import(child, scope)?,
                        // A package path binds names this model cannot see.
                        "module_path" => self.skip("module_path"),
                        _ => self.walk(child, scope)?,
                    }
                }
            }
            "for_expression" => {
                let inner = self.open(ScopeKind::For, scope, node)?;
                if let Some(clauses) = node.child_by_field_name("clauses") {
                    self.walk_for_clauses(clauses, inner, scope)?;
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.walk(body, inner)?;
                }
            }
            "if_expression" => {
                let inner = self.open(ScopeKind::If, scope, node)?;
                self.walk_children(node, inner)?;
            }
            "if_condition" => {
                // `if (Name := value)` declares `Name` for the branches.
                let mut cursor = node.walk();
                for (index, child) in node.children(&mut cursor).enumerate() {
                    if !child.is_named() {
                        continue;
                    }
                    if node.field_name_for_child(index as u32) == Some("name") {
                        self.declare(scope, child, BindingKind::Constant, false)?;
                    } else {
                        self.walk(child, scope)?;
                    }
                }
            }
            "attribute" => {
                // The attribute name is an annotation keyword, not a name.
                let mut cursor = node.walk();
                for (index, child) in node.children(&mut cursor).enumerate() {
                    if !child.is_named() || node.field_name_for_child(index as u32) == Some("name")
                    {
                        continue;
                    }
                    self.walk(child, scope)?;
                }
            }
            "member_access" => {
                let object = node.child_by_field_name("object");
                let member = node.child_by_field_name("member");
                let self_object = object.is_some_and(|object| {
                    object.kind() == "self_expression"
                        || (object.kind() == "identifier"
                            && matches!(self.text_of(object), Some("self" | "Self")))
                });
                if self_object {
                    // `self.Field` resolves against the enclosing class scope, not
                    // the method's local scopes, so a same-named local cannot
                    // capture it. `self` outside a class is not guessed.
                    if let Some(member) = member {
                        match self.enclosing_class(scope) {
                            Some(class) => self.reference_from(scope, Some(class), member)?,
                            None => self.skip("member_access"),
                        }
                    }
                } else {
                    // The member belongs to an unknown type; guessing it would
                    // be worse than admitting the gap.
                    self.skip("member_access");
                    if let Some(object) = object {
                        self.walk(object, scope)?;
                    }
                }
            }
            "field_initializer" | "named_argument" => {
                // Field and argument names are not declarative bindings.
                self.skip(node.kind());
                if let Some(value) = node.child_by_field_name("value") {
                    self.walk(value, scope)?;
                }
            }
            "case_expression" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.walk(value, scope)?;
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "case_arm" {
                        // Patterns match names this model does not bind.
                        self.skip("case_arm");
                    } else {
                        self.walk(child, scope)?;
                    }
                }
            }
            "parameter_list" | "tuple_parameter" => self.walk_parameters(node, scope)?,
            "positional_parameter"
            | "named_parameter"
            | "receiver_parameter"
            | "anonymous_parameter" => self.walk_parameter(node, scope)?,
            // Name-bearing constructs outside the working set.
            "module_scoped_identifier" | "qualified_type" | "where_clause" | "constrained_type" => {
                self.skip(node.kind());
            }
            "identifier" => self.reference(scope, node)?,
            // Pure structure: recurse and classify the names inside.
            "source_file"
            | "argument_list"
            | "field_initializer_list"
            | "field_initializer_block"
            | "array_literal"
            | "logic_literal"
            | "option_expression"
            | "struct_literal"
            | "object_construction"
            | "parenthesized_expression"
            | "tuple_expression"
            | "unary_expression"
            | "binary_expression"
            | "range_expression"
            | "query_expression"
            | "call_expression"
            | "failable_call_expression"
            | "index_expression"
            | "set_statement"
            | "loop_expression"
            | "when_expression"
            | "sync_expression"
            | "race_expression"
            | "branch_expression"
            | "spawn_expression"
            | "break_expression"
            | "return_expression"
            | "yield_expression"
            | "specifier_list"
            | "specifier"
            | "string_interpolation"
            | "generic_type"
            | "for_clause_list" => self.walk_children(node, scope)?,
            // Inert: no name to resolve.
            "line_comment"
            | "block_comment"
            | "hash_gt_comment"
            | "string_literal"
            | "string_content"
            | "escape_sequence"
            | "char_literal"
            | "integer_literal"
            | "float_literal"
            | "boolean_literal"
            | "path_literal"
            | "module_path"
            | "builtin_type"
            | "wildcard"
            | "self_expression"
            | "specifier_name"
            | "operator_function_name"
            | "prefix_function_name" => {}
            // Unknown or unclassified shape: count it and stop, so no name below
            // it is silently treated as unresolved.
            _ => self.skip(node.kind()),
        }
        Ok(())
    }

    fn walk_type_definition(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        let value = node.child_by_field_name("value");
        let boundary = match value.map(|value| value.kind()) {
            Some("class_definition" | "struct_definition" | "interface_definition") => {
                Some(ScopeKind::Class)
            }
            Some("module_definition") => Some(ScopeKind::Module),
            _ => None,
        };
        if let Some(name) = node.child_by_field_name("name") {
            // The pinned grammar also produces `type_definition` for the
            // immutable `Name := expression` binding form, so anything that is
            // not a type definition binds a constant.
            let kind = match value.map(|value| value.kind()) {
                Some("class_definition") => BindingKind::Class,
                Some("struct_definition") => BindingKind::Struct,
                Some("enum_definition") => BindingKind::Enum,
                Some("interface_definition") => BindingKind::Interface,
                // `type_definition` also covers the immutable `Name := value`
                // binding form, which is not a type alias.
                _ => BindingKind::Constant,
            };
            self.declare(scope, name, kind, false)?;
        }
        let target = match boundary {
            Some(kind) => self.open(kind, scope, node)?,
            None => scope,
        };
        for field in ["specifiers", "type_params"] {
            if let Some(child) = node.child_by_field_name(field) {
                self.walk(child, target)?;
            }
        }
        if let Some(value) = value {
            if boundary.is_some() {
                // Descend into the definition body without re-opening a scope,
                // and keep base types in the enclosing scope.
                self.walk_type_body(value, scope, target)?;
            } else {
                self.walk(value, target)?;
            }
        }
        Ok(())
    }

    /// Walk the members of a class/struct/interface definition into `inner`
    /// while evaluating base types against the enclosing `outer` scope: a base
    /// name is not a member of the type it derives from.
    fn walk_type_body(
        &mut self,
        node: Node<'_>,
        outer: usize,
        inner: usize,
    ) -> Result<(), Failure> {
        let mut cursor = node.walk();
        for (index, child) in node.children(&mut cursor).enumerate() {
            if !child.is_named() {
                continue;
            }
            let target = if node.field_name_for_child(index as u32) == Some("base") {
                outer
            } else {
                inner
            };
            self.walk(child, target)?;
        }
        Ok(())
    }

    fn walk_local_import(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        let mut cursor = node.walk();
        let segments: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "identifier")
            .collect();
        let Some((last, leading)) = segments.split_last() else {
            self.skip("local_module_path");
            return Ok(());
        };
        self.bind(scope, *last, BindingKind::ModuleImport, false)?;
        if !leading.is_empty() {
            // Enclosing module segments name a module this model cannot resolve.
            self.skip("local_module_path");
        }
        Ok(())
    }

    fn walk_for_clauses(
        &mut self,
        node: Node<'_>,
        inner: usize,
        outer: usize,
    ) -> Result<(), Failure> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "for_iterator" {
                for field in ["key", "value", "variable"] {
                    if let Some(name) = child.child_by_field_name(field) {
                        self.declare(inner, name, BindingKind::ForBinding, false)?;
                    }
                }
                // The iterable is evaluated before the bindings exist.
                if let Some(iterable) = child.child_by_field_name("iterable") {
                    self.walk(iterable, outer)?;
                }
            } else {
                self.walk(child, inner)?;
            }
        }
        Ok(())
    }

    fn walk_parameters(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "parameter_list" | "tuple_parameter" => self.walk_parameters(child, scope)?,
                "where_clause" => self.skip("where_clause"),
                _ => self.walk_parameter(child, scope)?,
            }
        }
        Ok(())
    }

    fn walk_parameter(&mut self, node: Node<'_>, scope: usize) -> Result<(), Failure> {
        let mut cursor = node.walk();
        for (index, part) in node.children(&mut cursor).enumerate() {
            if !part.is_named() {
                continue;
            }
            if node.field_name_for_child(index as u32) == Some("name") {
                self.declare(scope, part, BindingKind::Parameter, false)?;
            } else {
                self.walk(part, scope)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "semantic-tests.rs"]
mod tests;
