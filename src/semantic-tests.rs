//! Scope-graph tests. Included from `src/semantic.rs` as an inline module so the
//! private builder can be exercised without widening the public surface.

use super::*;

fn graph(text: &str) -> (Source, ScopeGraph) {
    let source = Source::from_bytes(text.as_bytes()).expect("source");
    let document = Document::parse(&source).expect("document");
    let model = ScopeGraph::build(&source, &document).expect("graph");
    (source, model)
}

/// What the `nth` reference to `name` resolves to, described by binding kind and
/// the declaration's own text, so no test depends on internal indices.
fn resolved(
    source: &Source,
    model: &ScopeGraph,
    name: &str,
    nth: usize,
) -> Option<(BindingKind, String)> {
    let reference = model
        .references()
        .iter()
        .filter(|r| r.name == name)
        .nth(nth)?;
    let binding = model.resolve(reference)?;
    Some((
        binding.kind,
        source.text()[binding.range.clone()].to_string(),
    ))
}

fn binding<'a>(model: &'a ScopeGraph, name: &str) -> &'a Binding {
    model
        .bindings()
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no binding named {name}"))
}

#[test]
fn retains_the_tree_without_changing_parse_outcomes() {
    let source = Source::from_bytes(b"F():void =\n    Print(\"x\")\n").unwrap();
    let document = Document::parse(&source).unwrap();
    assert_eq!(document.root().kind(), "source_file");
    assert_eq!(
        document.root().named_child(0).unwrap().kind(),
        "function_definition"
    );
    assert_eq!(document.body_start(), source.body_start());

    // Every input rejected before this slice still fails, from the same guards.
    for text in [
        "A := <p>text</p>\n",
        "得点 := 1\n",
        "A:int=\n",
        "A := map{\"x\" => 1}\n",
        "Add(X:int,Y:int):int = X+Y\n",
        "<# missing",
    ] {
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        assert!(Document::parse(&source).is_err(), "{text}");
    }
}

#[test]
fn local_variable_use_resolves_to_its_declaration() {
    let (source, model) = graph("F():void =\n    Value := 1\n    Print(Value)\n");
    assert_eq!(
        resolved(&source, &model, "Value", 0),
        Some((BindingKind::Constant, "Value".into()))
    );
    // `Print` is a call target this model cannot see; it stays unresolved.
    assert_eq!(resolved(&source, &model, "Print", 0), None);
}

#[test]
fn parameter_use_resolves_inside_the_function_only() {
    let (source, model) = graph("F(X:int):void =\n    Print(X)\n    Other := X\n");
    let x = binding(&model, "X");
    assert_eq!(x.kind, BindingKind::Parameter);
    assert_eq!(model.scopes()[x.scope].kind, ScopeKind::Function);
    for nth in 0..2 {
        assert_eq!(
            resolved(&source, &model, "X", nth),
            Some((BindingKind::Parameter, "X".into())),
            "reference {nth}"
        );
    }
    assert!(model.is_complete(), "{:?}", model.skipped());
}

#[test]
fn class_field_accessed_via_self_resolves_to_the_class_scope() {
    let (source, model) =
        graph("probe := class:\n    Value:int = 1\n    Get():int =\n        self.Value\n");
    let field = binding(&model, "Value");
    assert_eq!(field.kind, BindingKind::Constant);
    assert_eq!(model.scopes()[field.scope].kind, ScopeKind::Class);
    assert_eq!(
        resolved(&source, &model, "Value", 0),
        Some((BindingKind::Constant, "Value".into()))
    );
    // `self` is a keyword, not a name the model resolves.
    assert!(!model.references().iter().any(|r| r.name == "self"));
}

/// A local with the same name as a field must not capture `self.Field`: member
/// lookup starts at the class scope, not at the method's block scope.
#[test]
fn self_member_is_not_captured_by_a_same_named_local() {
    let (_, model) = graph(
        "shadowed := class:\n    Value:int = 1\n    Read():int =\n        Value := 2\n        self.Value\n",
    );
    let field = binding(&model, "Value");
    let class = field.scope;
    assert_eq!(model.scopes()[class].kind, ScopeKind::Class);

    // Exactly one `Value` reference: the `self.Value` read. It must resolve to
    // the class field, not to the same-named local.
    let reference = model
        .references()
        .iter()
        .find(|r| r.name == "Value")
        .expect("self.Value is a reference");
    let resolved_binding = model.resolve(reference).expect("resolves");
    assert_eq!(resolved_binding.scope, class);
    assert_eq!(resolved_binding.range, field.range);
    // The lexically enclosing scope is still recorded separately so a rule can
    // tell where the reference sits.
    assert_eq!(model.scopes()[reference.scope].kind, ScopeKind::Block);
    assert_eq!(reference.receiver, Some(class));
}

/// A field declared after the method that reads it is still visible: member
/// lookup is not a forward-only source scan.
#[test]
fn self_member_resolves_when_the_field_is_declared_later() {
    let (_, model) =
        graph("later := class:\n    Read():int =\n        self.Late\n    Late:int = 7\n");
    let field = binding(&model, "Late");
    let reference = model
        .references()
        .iter()
        .find(|r| r.name == "Late")
        .expect("self.Late is a reference");
    assert!(
        field.range.start > reference.range.start,
        "the field really is declared after the read"
    );
    assert_eq!(
        model.resolve(reference).map(|b| b.range.clone()),
        Some(field.range.clone())
    );
}

/// `self` outside a class is recorded as unsupported instead of resolving to
/// whatever local happens to share the member name.
#[test]
fn self_outside_a_class_is_unsupported_not_guessed() {
    let (_, model) = graph("F():void =\n    Field := 1\n    self.Field\n");
    assert!(!model.is_complete());
    assert!(model.reportable().is_none());
    assert_eq!(model.skipped().get("member_access"), Some(&1));
}

#[test]
fn inner_block_shadowing_wins_only_from_its_declaration_onward() {
    let text = "F():void =\n    Value := 1\n    if (true):\n        Value := 2\n        Print(Value)\n    Print(Value)\n";
    let (_, model) = graph(text);
    let declarations: Vec<usize> = model
        .bindings()
        .iter()
        .filter(|b| b.name == "Value")
        .map(|b| b.range.start)
        .collect();
    assert_eq!(declarations.len(), 2, "both declarations are bound");

    let inner = model
        .references()
        .iter()
        .find(|r| r.name == "Value")
        .and_then(|r| model.resolve(r))
        .expect("inner reference resolves");
    let outer = model
        .references()
        .iter()
        .filter(|r| r.name == "Value")
        .nth(1)
        .and_then(|r| model.resolve(r))
        .expect("outer reference resolves");

    assert_eq!(inner.range.start, declarations[1], "inner use sees inner");
    assert_eq!(outer.range.start, declarations[0], "outer use sees outer");
}

#[test]
fn name_used_before_redeclaration_keeps_the_outer_binding() {
    let text =
        "F():void =\n    Value := 1\n    if (true):\n        Print(Value)\n        Value := 2\n";
    let (_, model) = graph(text);
    let reference = model
        .references()
        .iter()
        .find(|r| r.name == "Value")
        .expect("reference");
    let resolved = model.resolve(reference).expect("resolves");
    let declarations: Vec<usize> = model
        .bindings()
        .iter()
        .filter(|b| b.name == "Value")
        .map(|b| b.range.start)
        .collect();
    // The reference precedes the inner declaration, so the outer one wins even
    // though the inner scope is nearer.
    assert_eq!(resolved.range.start, declarations[0]);
}

#[test]
fn self_reference_resolves_but_a_use_before_its_declaration_does_not() {
    let (source, model) =
        graph("F():void =\n    SelfRef := SelfRef\n    Print(Later)\n    Later := 1\n");
    let self_reference = model
        .references()
        .iter()
        .find(|reference| reference.name == "SelfRef")
        .expect("self-reference");
    let self_binding = model
        .bindings()
        .iter()
        .find(|binding| binding.name == "SelfRef")
        .expect("declaration");
    assert_eq!(
        model
            .resolve(self_reference)
            .map(|binding| binding.range.clone()),
        Some(self_binding.range.clone())
    );

    let before = model
        .references()
        .iter()
        .find(|reference| reference.name == "Later")
        .expect("use before declaration");
    assert!(
        model.resolve(before).is_none(),
        "a future same-scope declaration does not resolve: {:?}",
        &source.text()[before.range.clone()]
    );
    assert!(model.is_complete(), "{:?}", model.skipped());
}

#[test]
fn for_iterator_binding_resolves_in_the_body_only() {
    let (source, model) = graph(
        "F():void =\n    for (Key -> Value : Values):\n        Print(Value)\n    Print(Value)\n",
    );
    let value = binding(&model, "Value");
    assert_eq!(value.kind, BindingKind::ForBinding);
    assert_eq!(model.scopes()[value.scope].kind, ScopeKind::For);
    assert_eq!(
        resolved(&source, &model, "Value", 0),
        Some((BindingKind::ForBinding, "Value".into()))
    );
    // After the loop the name is not visible: unresolved, never guessed.
    assert_eq!(resolved(&source, &model, "Value", 1), None);
    assert_eq!(binding(&model, "Key").kind, BindingKind::ForBinding);
}

#[test]
fn range_iterator_binding_resolves_and_its_iterable_stays_outer() {
    let (source, model) =
        graph("F():void =\n    Values := array{1, 2}\n    for (I : Values):\n        Print(I)\n");
    let i = binding(&model, "I");
    assert_eq!(i.kind, BindingKind::ForBinding);
    assert_eq!(
        resolved(&source, &model, "I", 0),
        Some((BindingKind::ForBinding, "I".into()))
    );
    // The iterable is evaluated in the enclosing scope, before the binding exists.
    assert_eq!(
        resolved(&source, &model, "Values", 0),
        Some((BindingKind::Constant, "Values".into()))
    );
}

#[test]
fn if_condition_binding_resolves_inside_the_branches() {
    let (source, model) =
        graph("F():void =\n    if (Value := Get()):\n        Print(Value)\n    Print(Value)\n");
    let value = binding(&model, "Value");
    assert_eq!(value.kind, BindingKind::Constant);
    assert_eq!(model.scopes()[value.scope].kind, ScopeKind::If);
    assert_eq!(
        resolved(&source, &model, "Value", 0),
        Some((BindingKind::Constant, "Value".into()))
    );
    assert_eq!(resolved(&source, &model, "Value", 1), None);
}

#[test]
fn using_import_binds_the_local_module_name() {
    let (source, model) = graph("using { Demo.Helpers }\nF():void =\n    Print(Helpers)\n");
    let import = binding(&model, "Helpers");
    assert_eq!(import.kind, BindingKind::ModuleImport);
    assert_eq!(model.scopes()[import.scope].kind, ScopeKind::File);
    assert_eq!(
        resolved(&source, &model, "Helpers", 0),
        Some((BindingKind::ModuleImport, "Helpers".into()))
    );
    // A qualified import names a module the model cannot resolve, so it is
    // recorded as unsupported rather than silently bound.
    assert!(!model.is_complete());
    assert_eq!(model.skipped().get("local_module_path"), Some(&1));
}

#[test]
fn out_of_scope_reference_is_unresolved_not_guessed() {
    let (source, model) = graph("F():void =\n    Inner := 1\nG():void =\n    Print(Inner)\n");
    assert_eq!(resolved(&source, &model, "Inner", 0), None);
    assert!(model.is_complete(), "{:?}", model.skipped());
}

#[test]
fn declaration_kinds_are_classified_by_construct() {
    let (_, model) = graph(
        "Count:int = 1\nvar Total:int = 0\nprobe := class:\n    Field:int = 1\n    Get():int =\n        Field\nshade := enum{Light}\npoint := struct:\n    X:int = 0\nface := interface:\n    F():void\n",
    );
    let kind = |name: &str| (binding(&model, name).kind, binding(&model, name).mutable);
    assert_eq!(kind("Count"), (BindingKind::Constant, false));
    assert_eq!(kind("Total"), (BindingKind::Variable, true));
    assert_eq!(kind("probe"), (BindingKind::Class, false));
    assert_eq!(kind("Field"), (BindingKind::Constant, false));
    assert_eq!(kind("Get"), (BindingKind::Function, false));
    assert_eq!(kind("shade"), (BindingKind::Enum, false));
    assert_eq!(kind("point"), (BindingKind::Struct, false));
    assert_eq!(kind("face"), (BindingKind::Interface, false));
}

#[test]
fn graph_is_deterministic_across_repeated_builds() {
    let text = "using { Demo.Helpers }\nF(X:int):void =\n    Value := X\n    for (K -> V : Values):\n        Print(V)\n    Print(Value)\n";
    let view = |model: &ScopeGraph| {
        (
            model
                .scopes()
                .iter()
                .map(|s| (s.kind, s.parent))
                .collect::<Vec<_>>(),
            model
                .bindings()
                .iter()
                .map(|b| (b.name.clone(), b.kind, b.scope, b.range.clone(), b.mutable))
                .collect::<Vec<_>>(),
            model
                .references()
                .iter()
                .map(|r| (r.name.clone(), r.range.clone(), r.scope))
                .collect::<Vec<_>>(),
        )
    };
    let (_, first) = graph(text);
    let (_, second) = graph(text);
    assert_eq!(view(&first), view(&second));
    for text in [
        "A := 1\n",
        "F():void =\n    Print(\"x\")\n",
        "using { /Fortnite.com/Devices }\n",
    ] {
        let (_, a) = graph(text);
        let (_, b) = graph(text);
        assert_eq!(view(&a), view(&b), "{text}");
    }
}

#[test]
fn unsupported_constructs_make_the_graph_incomplete_and_unreportable() {
    // An object-construction field initializer names a field on a type this
    // model cannot see, so the file becomes unreportable.
    let (_, model) = graph("F():void =\n    P := point{X := 1}\n    Print(P)\n");
    assert!(!model.is_complete());
    assert!(model.reportable().is_none());
    assert_eq!(model.skipped().get("field_initializer"), Some(&1));

    let (_, member) = graph("F():void =\n    Print(other.Value)\n");
    assert!(member.is_complete(), "{:?}", member.skipped());
    // A non-self member is deliberately not a lexical reference; its object is.
    assert_eq!(
        member
            .references()
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["Print", "other"]
    );
    assert_eq!(
        member
            .unresolved(&[])
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["other"]
    );

    // A file with only covered constructs stays complete and reportable, which
    // keeps the assertions above meaningful instead of vacuous.
    let (_, complete) = graph("F(X:int):void =\n    Print(X)\n");
    assert!(complete.is_complete());
    assert!(complete.reportable().is_some());
}

#[test]
fn skipped_kinds_are_counted_per_kind_within_the_bound() {
    let (_, model) = graph(
        "F(object:int):void =\n    G(?Name := 1)\n    Print(object.Field)\n    H := point{X := 1}\n",
    );
    assert_eq!(model.skipped().get("named_argument"), Some(&1));
    assert_eq!(model.skipped().get("field_initializer"), Some(&1));
    assert!(!model.is_complete());
    assert!(model.skipped().len() <= MAX_SKIPPED_KINDS);
}

#[test]
fn file_scope_is_the_root_and_every_other_scope_has_a_parent() {
    let (_, model) = graph("A := 1\nF():void =\n    B := 2\n    Print(B)\n");
    assert_eq!(model.scopes()[0].kind, ScopeKind::File);
    assert_eq!(model.scopes()[0].parent, None);
    for scope in &model.scopes()[1..] {
        let parent = scope.parent.expect("child scope has a parent");
        assert!(parent < model.scopes().len());
    }
    // Per-scope views agree with the flat lists they describe.
    for (index, scope) in model.scopes().iter().enumerate() {
        let owned: Vec<&str> = model.bindings_in(index).map(|b| b.name.as_str()).collect();
        let flat: Vec<&str> = model
            .bindings()
            .iter()
            .filter(|b| b.scope == index)
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(owned, flat, "bindings of scope {index} ({:?})", scope.kind);
        let here = model.references_in(index).count();
        let all = model
            .references()
            .iter()
            .filter(|r| r.scope == index)
            .count();
        assert_eq!(here, all, "references of scope {index}");
    }
}

/// Names of the bindings [`ScopeGraph::unused`] returns, in declaration order.
fn unused_names(model: &ScopeGraph, parameters: bool) -> Vec<&str> {
    model
        .unused(parameters)
        .into_iter()
        .map(|binding| binding.name.as_str())
        .collect()
}

#[test]
fn unused_reports_function_body_locals_and_nothing_else() {
    let (_, model) = graph(concat!(
        "Top:int = 1\n",
        "F(Param:int):void =\n",
        "    Local:int = 2\n",
        "    Read:int = 3\n",
        "    Print(Read)\n",
    ));
    assert_eq!(unused_names(&model, false), ["Local"]);
    // The file-scope binding is reachable from elsewhere and the parameter is
    // excluded until the switch asks for it.
    assert_eq!(unused_names(&model, true), ["Param", "Local"]);
}

#[test]
fn unused_treats_exports_and_the_underscore_marker_as_used() {
    let (_, model) = graph(concat!(
        "probe<public> := class:\n",
        "    Field<public>:int = 1\n",
        "    Get():void =\n",
        "        Local:int = 1\n",
    ));
    // `Get` is not exported but it is a class member, not a body local: only
    // `Local` qualifies, and the public names are excluded by `exported`.
    assert_eq!(unused_names(&model, false), ["Local"]);
    let (_, marker) = graph("F():void =\n    _Skipped:int = 1\n");
    assert_eq!(unused_names(&marker, false), Vec::<&str>::new());
    assert_eq!(unused_names(&marker, true), Vec::<&str>::new());
}

#[test]
fn exported_is_recorded_per_binding_from_the_declaration_specifiers() {
    let (_, model) = graph(concat!(
        "probe<public> := class:\n",
        "    Visible<public>:int = 1\n",
        "    Hidden:int = 2\n",
    ));
    // The top-level type name is exported because it is the module surface.
    // `Name := class<public>` would put the specifier on the value instead, so
    // both shapes must reach the same answer.
    assert!(binding(&model, "probe").exported);
    assert!(binding(&model, "Visible").exported);
    assert!(!binding(&model, "Hidden").exported);
    let (_, value_side) = graph("probe := class<public>:\n    Field:int = 1\n");
    assert!(binding(&value_side, "probe").exported);
}

#[test]
fn unused_is_a_documented_no_op_on_an_incomplete_graph() {
    let (_, model) =
        graph("F():void =\n    Local:int = 1\n    P := point{X := 1}\n    Print(Local)\n");
    assert!(!model.is_complete());
    // Even with parameters enabled the incomplete graph reports nothing, which
    // is the invariant the rule layer relies on.
    assert!(model.unused(false).is_empty());
    assert!(model.unused(true).is_empty());
    assert!(model.reportable().is_none());
}

#[test]
fn unused_respects_shadowing_and_nested_block_ownership() {
    let (_, model) = graph(concat!(
        "F():void =\n",
        "    Value:int = 1\n",
        "    if (true):\n",
        "        Value:int = 2\n",
        "    Print(Value)\n",
    ));
    // The inner declaration is the unreferenced one; the outer is read after the
    // block, so exactly one binding is reported and it is the inner one.
    let reported = model.unused(false);
    assert_eq!(reported.len(), 1);
    assert!(reported[0].range.start > 20, "{}", reported[0].range.start);
}

#[test]
fn for_and_if_bindings_are_never_unused_candidates() {
    let (_, model) = graph(concat!(
        "F():void =\n",
        "    for (Key -> Value : Values):\n",
        "        Print(\"x\")\n",
        "    if (Bound := Get()):\n",
        "        Print(\"y\")\n",
    ));
    // The bound value is tested or iterated by the construct that binds it; that
    // is not evidence the declaration is dead.
    assert_eq!(unused_names(&model, true), Vec::<&str>::new());
}
