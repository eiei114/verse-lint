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

/// Names of the bindings [`ScopeGraph::shadowing`] returns, in declaration order.
fn shadowing_names<'a>(model: &'a ScopeGraph, allow: &[&str]) -> Vec<&'a str> {
    let allow: Vec<String> = allow.iter().map(|name| (*name).to_owned()).collect();
    model
        .shadowing(&allow)
        .into_iter()
        .map(|binding| binding.name.as_str())
        .collect()
}

/// Byte offsets of every binding named `name`, in declaration order.
fn declarations(model: &ScopeGraph, name: &str) -> Vec<usize> {
    model
        .bindings()
        .iter()
        .filter(|binding| binding.name == name)
        .map(|binding| binding.range.start)
        .collect()
}

#[test]
fn an_inner_local_that_shadows_an_outer_local_is_one_finding() {
    let (_, model) = graph(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "        Print(Value)\n",
        "    Print(Value)\n",
    ));
    let reported = model.shadowing(&[]);
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].name, "Value");
    // The finding points at the inner declaration: the text inside the reported
    // range is the name itself, and nothing about the outer binding.
    let offsets = declarations(&model, "Value");
    assert_eq!(offsets.len(), 2, "both declarations are bound");
    assert_eq!(reported[0].range.start, offsets[1]);
    assert!(offsets[1] > offsets[0]);
}

#[test]
fn a_local_shadowing_a_parameter_is_reported_but_the_idiomatic_construct_binding_is_not() {
    // `Param := 1` inside the body reuses the parameter's name: a real capture.
    let (_, capture) = graph("F(Param:int):void =\n    Param := 1\n    Print(Param)\n");
    assert_eq!(shadowing_names(&capture, &[]), ["Param"]);
    // `for (Param : ...)` rebinding the same function's own parameter is the
    // shape Verse iteration uses; reporting it would only add noise.
    let (_, iteration) = graph(concat!(
        "F(Param:int):void =\n",
        "    Values := array{1}\n",
        "    for (Param : Values):\n",
        "        Print(Param)\n",
    ));
    assert_eq!(shadowing_names(&iteration, &[]), Vec::<&str>::new());
    let (_, condition) = graph(concat!(
        "F(Param:int):void =\n",
        "    if (Param := Get()):\n",
        "        Print(Param)\n",
    ));
    assert_eq!(shadowing_names(&condition, &[]), Vec::<&str>::new());
    // The exemption is about the immediate function's own parameter, so a
    // construct binding that captures a *local* is still reported.
    let (_, local) = graph(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    for (Value : Values):\n",
        "        Print(Value)\n",
    ));
    assert_eq!(shadowing_names(&local, &[]), ["Value"]);
}

#[test]
fn shadowing_a_type_function_or_module_import_name_is_left_to_a_later_slice() {
    // The model has no namespaces, so a local spelled like a type, a function or
    // a single-segment module import is not claimed as shadowing.
    for text in [
        "probe := class:\n    Field:int = 1\nF():void =\n    probe := 1\n    Print(probe)\n",
        "F():void =\n    Print(\"x\")\nG():void =\n    F := 1\n    Print(F)\n",
        "using { Helpers }\nF():void =\n    Helpers := 1\n    Print(Helpers)\n",
        "shade := enum{Light}\nF():void =\n    shade := 1\n    Print(shade)\n",
        "point := struct:\n    X:int = 0\nF():void =\n    point := 1\n    Print(point)\n",
        "face := interface:\n    F():void\nG():void =\n    face := 1\n    Print(face)\n",
    ] {
        let (_, model) = graph(text);
        assert_eq!(shadowing_names(&model, &[]), Vec::<&str>::new(), "{text}");
    }
    // Control: the same shape between two value names is reported, so the
    // silence above is the namespace exclusion and nothing else.
    let (_, control) = graph("Value := 1\nF():void =\n    Value := 2\n    Print(Value)\n");
    assert_eq!(shadowing_names(&control, &[]), ["Value"]);
}

#[test]
fn a_same_scope_redeclaration_is_not_shadowing() {
    let (_, model) = graph("F():void =\n    Value := 1\n    Value := 2\n    Print(Value)\n");
    assert_eq!(shadowing_names(&model, &[]), Vec::<&str>::new());
    // Two parameters of one function are also a same-scope pair.
    let (_, parameters) = graph("F(Value:int, Value:int):void =\n    Print(Value)\n");
    assert_eq!(shadowing_names(&parameters, &[]), Vec::<&str>::new());
}

#[test]
fn an_underscore_name_and_the_allowlist_are_never_reported() {
    let (_, marker) = graph(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        _Value := 2\n",
        "        Print(_Value)\n",
    ));
    assert_eq!(shadowing_names(&marker, &[]), Vec::<&str>::new());
    let (_, allowed) = graph("Value := 1\nF():void =\n    Value := 2\n    Print(Value)\n");
    assert_eq!(shadowing_names(&allowed, &[]), ["Value"]);
    // An allow entry is exact and case-sensitive, and it silences the pair.
    assert_eq!(shadowing_names(&allowed, &["Value"]), Vec::<&str>::new());
    assert_eq!(shadowing_names(&allowed, &["value"]), ["Value"]);
}

#[test]
fn an_inner_binding_shadowing_two_ancestors_yields_one_finding_for_the_nearest() {
    let (_, model) = graph(concat!(
        "Value := 1\n",
        "F():void =\n",
        "    Value := 2\n",
        "    if (true):\n",
        "        Value := 3\n",
        "        Print(Value)\n",
    ));
    let offsets = declarations(&model, "Value");
    assert_eq!(offsets.len(), 3, "all three declarations are bound");
    // Two inner declarations, one finding each: the function-body one shadows the
    // file-scope one, and the block one shadows the function-body one. The
    // nearest ancestor is what the search returns, so a second diagnostic for a
    // farther ancestor cannot exist.
    let reported = model.shadowing(&[]);
    assert_eq!(
        reported
            .iter()
            .map(|binding| binding.range.start)
            .collect::<Vec<_>>(),
        offsets[1..]
    );
}

#[test]
fn shadowing_reports_nothing_from_an_incomplete_graph_and_nothing_out_of_order() {
    let (_, incomplete) = graph(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "    P := point{X := 1}\n",
    ));
    assert!(incomplete.shadowing(&[]).is_empty());
    assert!(!incomplete.is_complete());
    assert!(incomplete.reportable().is_none());
    // An ancestor declared after the inner binding captured nothing, so the pair
    // is not reported: resolution only counts a declaration that already started.
    let (_, later) = graph(concat!(
        "F():void =\n",
        "    if (true):\n",
        "        Value := 2\n",
        "    Value := 1\n",
    ));
    assert!(later.shadowing(&[]).is_empty());
    // A name declared only in a sibling scope is not an ancestor.
    let (_, sibling) = graph(concat!(
        "F():void =\n",
        "    if (true):\n",
        "        Value := 1\n",
        "    if (false):\n",
        "        Value := 2\n",
    ));
    assert!(sibling.shadowing(&[]).is_empty());
}

/// A parameter -> local -> inner-block chain: the depth of the chain does not
/// change the claim, each inner declaration is reported once, and the parameter
/// is only ever the ancestor.
#[test]
fn a_parameter_local_inner_block_chain_reports_each_inner_declaration_once() {
    let (_, model) = graph(concat!(
        "F(Value:int):void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "        Print(Value)\n",
        "    Print(Value)\n",
        "    Print(Value)\n",
    ));
    let offsets = declarations(&model, "Value");
    assert_eq!(offsets.len(), 3, "parameter, local, inner local");
    let reported = model.shadowing(&[]);
    assert_eq!(
        reported
            .iter()
            .map(|binding| binding.range.start)
            .collect::<Vec<_>>(),
        offsets[1..]
    );
    assert!(
        reported
            .iter()
            .all(|binding| binding.kind != BindingKind::Parameter)
    );
}

/// A construct that becomes parseable but is not modelled must make the graph
/// incomplete, so no semantic rule can report from it. A map literal is exactly
/// that shape today: the CST is complete, the walker classifies `map_literal`
/// as unsupported, and the file therefore becomes unreportable instead of
/// silently resolving to "no references".
#[test]
fn a_parseable_map_literal_makes_the_graph_incomplete() {
    let (_, model) = graph("F():void =\n    M := map{\"a\" => 1}\n    Print(M)\n");
    assert_eq!(model.skipped().get("map_literal"), Some(&1));
    assert!(!model.is_complete());
    assert!(model.reportable().is_none());
    // The binding and the surrounding references still exist, which keeps the
    // assertion above about `is_complete` rather than about a failed parse.
    assert_eq!(
        binding(&model, "M").kind,
        BindingKind::Constant,
        "the local is still bound"
    );
    assert_eq!(
        model
            .references()
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["Print", "M"]
    );

    // Control: the same function without the map literal stays complete, so the
    // incompleteness above is the map literal and nothing else.
    let (_, complete) = graph("F():void =\n    M := 1\n    Print(M)\n");
    assert!(complete.is_complete());
    assert!(complete.reportable().is_some());
}

/// A string literal that starts with `#` is inert as far as the scope graph is
/// concerned: it is a literal, not a name-bearing construct, so it must not
/// clear `is_complete()` merely because it now parses.
#[test]
fn a_hash_prefixed_string_stays_a_complete_graph() {
    let (source, model) = graph("F():void =\n    L := \"# text\"\n    Print(L)\n");
    assert!(model.is_complete(), "{:?}", model.skipped());
    assert_eq!(
        resolved(&source, &model, "L", 0),
        Some((BindingKind::Constant, "L".into()))
    );
}

/// `map{}` with no entries does not become a `map_literal`: it keeps the older
/// `object_construction` reading, and the walker treats that node as pure
/// structure. The graph therefore stays complete with no skipped kind, and the
/// head is an ordinary `map` reference that resolves only when the project binds
/// that name — which is what `docs/parser-design.md` documents for the entry-less
/// form. This test pins both the dump and the reference, so the prose cannot drift
/// back to claiming the entry-less map is counted as unsupported.
#[test]
fn an_entry_less_map_keeps_a_complete_graph_and_a_plain_reference() {
    let text = "A := map{}\n";
    let (source, model) = graph(text);

    // The CST reading behind the doc claim: the entry-less braced head is an
    // `object_construction` carrying the `map` identifier, never `map_literal`.
    let document = Document::parse(&source).expect("document");
    let mut kinds = Vec::new();
    collect_kinds(document.root(), &mut kinds);
    assert!(
        kinds.iter().any(|kind| kind == "object_construction"),
        "{kinds:?}"
    );
    assert!(
        !kinds.iter().any(|kind| kind == "map_literal"),
        "an entry-less head must not become a map literal: {kinds:?}"
    );

    // Dump shape: complete, nothing skipped, `map` a plain unresolved reference.
    let dump: serde_json::Value = serde_json::from_str(
        &GraphDump::new(vec![model.dump("empty-map.verse")])
            .to_json()
            .expect("dump json"),
    )
    .expect("dump parses");
    let file = &dump["files"][0];
    assert_eq!(file["isComplete"], serde_json::Value::Bool(true), "{dump}");
    assert_eq!(file["skipped"], serde_json::json!({}), "{dump}");
    assert_eq!(file["bindings"][0]["name"], "A", "{dump}");
    assert_eq!(file["bindings"][0]["kind"], "constant", "{dump}");
    let head = &file["references"][0];
    assert_eq!(head["name"], "map", "{dump}");
    assert_eq!(head["range"], serde_json::json!({ "start": 5, "end": 8 }));
    assert_eq!(head["resolved"], serde_json::Value::Null, "{dump}");
    assert_eq!(model.unresolved(&[]).len(), 1, "V3002 still has a name");

    // The same head resolves once the project binds `map`, so the assertion
    // above is about the name being unbound, not about the construct.
    let (_, bound) = graph("map := 1\nA := map{}\n");
    let head = bound
        .references()
        .iter()
        .find(|reference| reference.name == "map")
        .expect("head reference");
    assert!(bound.resolve(head).is_some(), "a bound `map` resolves");
}

fn collect_kinds(node: tree_sitter::Node<'_>, kinds: &mut Vec<String>) {
    kinds.push(node.kind().to_owned());
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_kinds(child, kinds);
    }
}
