//! An AnimBP's anim graph as the body part reads it: the anim node properties
//! of an AnimBlueprintGeneratedClass and their class default values, the exposed input copies
//! (PropertyAccessLibrary), and the slot / state machine / node analysis the game's anim code reads (`slotHolds`,
//! `animNodes`).
//!
//! The graph: the anim node properties of the class in forward order (a pose link's LinkID L is the L-th of them), the
//! node values in the class default object's own properties. A pose link is any `{LinkID: n}` inside a node's value; a
//! UseCachedPose node links to its SaveCachedPose node, which is updated when any consumer is.
//!
//! FAnimNode_Slot::Update_AnyThread (exe 1424945f0) updates its Source link only with bAlwaysUpdateSourcePose or a
//! source weight above 0.00001: under a montage at full weight with a track in the node's slot nothing below the node
//! is updated through it. `slot_holds` answers the part of "is the locomotion updated this tick" that the graph alone
//! states.
//!
//! Targets: the LinkedAnimLayer / LinkedAnimGraph nodes reachable from the Root named AnimGraph through pose links,
//! and the StateMachine nodes one of whose state graphs reaches such a node (a state's graph is not linked from the
//! machine's node, so the machine is the leaf).
//!
//! cuts(node) is the list of slot sets, each of which stops every update from that node down to a target:
//!   - a target: none (nothing below it to stop);
//!   - a Slot node without the flag, name S, over X: [S] and every cut of X;
//!   - a Slot node with the flag: the cuts of X;
//!   - any other node: one cut of each input that reaches a target, joined (every combination);
//!   - an input that reaches no target does not count.
//!
//! Only the minimal sets are kept. Every input of a blend node is taken as live, so a set listed here holds the
//! locomotion whatever the blend inputs are; a hold that depends on a blend's input is not listed and is the
//! character's own code.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;
use mvs_data::{Props, Value};

use super::bag::{arr_of, at, b_or, n_or, num, obj, or, or_empty, s, s_or, some, str_of, tail, truthy};
use crate::kind::{Cx, KindError};
use crate::kinds::gamedata::shape::{cdo_path, fail, native, properties, short_name};

/// An AnimBP class and its class default object, as the graph readers take them (the bag of each).
pub struct AnimBp {
    /// The class's object path.
    pub path: String,
    /// The class's tagged properties.
    pub cls: Props,
    /// What the class's native serializer read (ChildProperties, SuperStruct).
    pub native: Props,
    /// The class default object's own properties.
    pub cdo: Props,
}

impl AnimBp {
    pub fn load(cx: &Cx<'_>, cls: &str) -> Result<AnimBp, KindError> {
        let c = cx.object(cls)?;
        let d = cx.object(&cdo_path(cls))?;
        Ok(AnimBp {
            path: c.path().to_owned(),
            cls: properties(&c)?,
            native: native(&c)?.unwrap_or_default(),
            cdo: properties(&d)?,
        })
    }

    /// The class's name (`Shaggy_AnimBP_C`).
    pub fn name(&self) -> &str {
        short_name(&self.path)
    }

    /// The class default object's own value of a property (`{}` when it stores none).
    fn own(&self, name: &str) -> Value {
        or(self.cdo.get(name), Value::Obj(Props::new()))
    }
}

/// A declared property of a class (an entry of its ChildProperties).
pub type ChildProp = Props;

/// The class's declared properties (its ChildProperties), in forward order.
pub fn child_props(native: &Props) -> Vec<&ChildProp> {
    arr_of(native.get("ChildProperties")).iter().filter_map(Value::as_obj).collect()
}

/// A property's name.
pub fn prop_name(c: &ChildProp) -> &str {
    str_of(c.get("Name")).unwrap_or_default()
}

/// A node property's struct name (`/Script/Engine.AnimNode_Root` -> `AnimNode_Root`), '' for none.
pub fn struct_of(c: &ChildProp) -> &str {
    str_of(c.get("Struct")).map_or("", short_name)
}

/// The anim node properties (the project's MirrorNode struct is one: it has no AnimNode_ in its name).
pub fn anim_node_props(native: &Props) -> Vec<&ChildProp> {
    child_props(native)
        .into_iter()
        .filter(|c| {
            let s = struct_of(c);
            s.contains("AnimNode_") || s.contains("MirrorNode")
        })
        .collect()
}

/// Whether a value is a pose link (`{LinkID: n}` alone).
fn is_link(x: &Value) -> bool {
    x.as_obj().is_some_and(|o| o.len() == 1 && o.contains_key("LinkID"))
}

fn link_id(x: &Value) -> f64 {
    n_or(at(x, "LinkID"), f64::NAN)
}

/// The pose links inside a value, in order.
fn links(v: &Value, out: &mut Vec<usize>) {
    match v {
        Value::Arr(a) => a.iter().for_each(|x| links(x, out)),
        Value::Obj(o) => {
            if is_link(v) {
                let l = link_id(v);
                if l >= 0.0 {
                    out.push(l as usize);
                }
                return;
            }
            o.values().for_each(|x| links(x, out));
        }
        _ => {}
    }
}

/// The pose links inside a value with their property path (`BlendPose[0]`, `Source`), in order.
fn named_links(v: &Value) -> Vec<(String, usize)> {
    fn walk(x: &Value, p: &str, out: &mut Vec<(String, usize)>) {
        match x {
            Value::Arr(a) => a.iter().enumerate().for_each(|(i, y)| walk(y, &format!("{p}[{i}]"), out)),
            Value::Obj(o) => {
                if is_link(x) {
                    let l = link_id(x);
                    if l >= 0.0 {
                        out.push((p.to_owned(), l as usize));
                    }
                    return;
                }
                for (k, y) in o {
                    walk(y, &if p.is_empty() { k.clone() } else { format!("{p}.{k}") }, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, "", &mut out);
    out
}

/// The PropertyAccessLibrary's copies of every copy batch (CopyBatches is a fixed array).
fn copies_of(lib: Option<&Value>) -> Vec<Value> {
    let Some(batches) = some(lib.and_then(|l| at(l, "CopyBatches"))) else { return Vec::new() };
    let list: Vec<&Value> = match batches {
        Value::Arr(a) => a.iter().collect(),
        b => vec![b],
    };
    list.into_iter().flat_map(|b| arr_of(at(b, "Copies")).to_vec()).collect()
}

fn cmp_list(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match x.cmp(y) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    a.len().cmp(&b.len())
}

/// The path a PropertyAccessLibrary path names: its segments' names.
fn segments(lib: &Value, q: &Value) -> Vec<String> {
    let segs = arr_of(at(lib, "PathSegments"));
    let start = n_or(at(q, "PathSegmentStartIndex"), 0.0) as usize;
    let count = n_or(at(q, "PathSegmentCount"), 0.0) as usize;
    segs.iter().skip(start).take(count).map(|s| str_of(at(s, "Name")).unwrap_or_default().to_owned()).collect()
}

fn index(list: &[Value], i: Option<&Value>, what: &str) -> Result<Value, KindError> {
    let n = n_or(i, f64::NAN);
    if n >= 0.0 && (n as usize) < list.len() {
        Ok(list[n as usize].clone())
    } else {
        fail(format!("{what}: index {n} out of range"))
    }
}

/// An anim node's exposed-input copies (the class's PropertyAccessLibrary), sorted: [source path, destination path
/// under the node]; `None` where the node has a bound function, no handler or a copy with a post-copy operation.
pub fn exposed_inputs(cls: &Props, node: &str) -> Result<Option<Vec<[String; 2]>>, KindError> {
    let Some(lib) = cls.get("PropertyAccessLibrary") else { return fail("exposedInputs: no PropertyAccessLibrary") };
    let batches = at(lib, "CopyBatches");
    let copies = match batches {
        Some(Value::Arr(a)) => arr_of(a.first().and_then(|b| at(b, "Copies"))),
        b => arr_of(b.and_then(|b| at(b, "Copies"))),
    };
    let seg = |q: &Value| segments(lib, q).join(".");
    let h: Vec<&Value> = arr_of(cls.get("EvaluateGraphExposedInputs"))
        .iter()
        .filter(|e| {
            str_of(at(e, "ValueHandlerNodeProperty").and_then(|v| at(v, "Path")).and_then(|p| p.as_arr()?.first()))
                == Some(node)
        })
        .collect();
    let Some(h0) = h.first() else { return Ok(None) };
    if s_or(at(h0, "BoundFunction"), "None") != "None" {
        return Ok(None);
    }
    let mut out: Vec<[String; 2]> = Vec::new();
    for r in arr_of(at(h0, "CopyRecords")) {
        if s_or(at(r, "PostCopyOperation"), "EPostCopyOperation::None") != "EPostCopyOperation::None" {
            return Ok(None);
        }
        let c = index(copies, at(r, "CopyIndex"), "exposedInputs copy")?;
        let from = n_or(at(&c, "DestAccessStartIndex"), 0.0) as usize;
        let to = n_or(at(&c, "DestAccessEndIndex"), 0.0) as usize;
        for k in from..to {
            let dest = seg(&index(arr_of(at(lib, "DestPaths")), Some(&num(k as f64)), "exposedInputs dest")?);
            let Some(under) = dest.strip_prefix(node).and_then(|d| d.strip_prefix('.')) else { return Ok(None) };
            let src = seg(&index(arr_of(at(lib, "SrcPaths")), at(&c, "AccessIndex"), "exposedInputs source")?);
            out.push([src, under.to_owned()]);
        }
    }
    out.sort_by(|a, b| cmp_list(a, b));
    Ok(Some(out))
}

/// The graph's exposed inputs by (node, property): a property access copy (the AnimBP variable path it reads,
/// '.'-joined) or a bound function (the node's own name under property '', as nothing in the data says what the
/// function computes). Keys `node \0 property`, in the order first set.
type Bindings = IndexMap<String, String>;

fn bkey(node: &str, prop: Option<&str>) -> String {
    format!("{node}\0{}", prop.unwrap_or_default())
}

fn bindings(bp: &AnimBp) -> Result<Bindings, KindError> {
    let cp = &bp.cls;
    let mut out = Bindings::new();
    let empty = Value::Obj(Props::new());
    let pal = some(cp.get("PropertyAccessLibrary")).unwrap_or(&empty);
    let copies = copies_of(Some(pal));
    let src = arr_of(at(pal, "SrcPaths"));
    let dst = arr_of(at(pal, "DestPaths"));
    for e in arr_of(cp.get("EvaluateGraphExposedInputs")) {
        let node =
            str_of(at(e, "ValueHandlerNodeProperty").and_then(|v| at(v, "Path")).and_then(|p| p.as_arr()?.first()))
                .unwrap_or_default()
                .to_owned();
        let bf = at(e, "BoundFunction");
        if some(bf).is_some() && str_of(bf) != Some("None") {
            out.insert(bkey(&node, None), node.clone());
        }
        for r in arr_of(at(e, "CopyRecords")) {
            let cpy = index(&copies, at(r, "CopyIndex"), "animgraph copy")?;
            let d = segments(pal, &index(dst, at(&cpy, "DestAccessStartIndex"), "animgraph dest")?);
            if d.len() != 2 || d[0] != node {
                return fail(format!(
                    "animgraph: {}: a copy into {} that is not one property of the node",
                    bp.name(),
                    d.join(".")
                ));
            }
            let from = segments(pal, &index(src, at(&cpy, "AccessIndex"), "animgraph source")?).join(".");
            out.insert(bkey(&node, Some(&d[1])), from);
        }
    }
    Ok(out)
}

fn bound(b: &Bindings, node: &str, prop: &str) -> Value {
    b.get(&bkey(node, Some(prop))).or_else(|| b.get(&bkey(node, None))).map_or(Value::Null, |x| s(x.clone()))
}

/// The state machines of the AnimBP by name: {reinit: bReinitializeOnBecomingRelevant, skipFirst:
/// bSkipFirstUpdateTransition, entry: the entry state's name}, as the class default object has the node.
/// FAnimNode_StateMachine::Update_AnyThread (exe 142867fa0): a machine with the first flag starts again at its entry
/// state in the first update after a missed one. A class with two copies of a graph (Base_AnimBP: the AnimGraph and
/// the BaseMovementMachine layer) has two nodes a name; they have to agree. No default for a flag the node's value
/// does not state.
pub fn state_machines(bp: &AnimBp) -> Result<Value, KindError> {
    let name0 = bp.name();
    let baked = arr_of(bp.cls.get("BakedStateMachines"));
    let mut out = Props::new();
    for c in child_props(&bp.native) {
        if !str_of(c.get("Struct")).unwrap_or_default().contains("AnimNode_StateMachine") {
            continue;
        }
        let v = bp.own(prop_name(c));
        let idx = n_or(at(&v, "StateMachineIndexInClass"), -1.0);
        let vo = v.as_obj().cloned().unwrap_or_default();
        if !(idx >= 0.0 && (idx as usize) < baked.len())
            || !vo.contains_key("bReinitializeOnBecomingRelevant")
            || !vo.contains_key("bSkipFirstUpdateTransition")
        {
            return fail(format!(
                "animgraph: {name0}.{}: a state machine node without its baked machine or its flags",
                prop_name(c)
            ));
        }
        let b = &baked[idx as usize];
        let states = arr_of(at(b, "States"));
        let ini = n_or(at(b, "InitialState"), -1.0);
        let machine = str_of(at(b, "MachineName")).unwrap_or_default();
        if !(ini >= 0.0 && (ini as usize) < states.len()) {
            return fail(format!("animgraph: {name0} machine {machine} has no entry state"));
        }
        // (Base_AnimBP's layer copy of RunSubState is named RunSubStateAnimLayer)
        let name = machine.strip_suffix("AnimLayer").unwrap_or(machine).to_owned();
        let m = obj([
            ("reinit", Value::Bool(truthy(vo.get("bReinitializeOnBecomingRelevant")))),
            ("skipFirst", Value::Bool(truthy(vo.get("bSkipFirstUpdateTransition")))),
            ("entry", or(at(&states[ini as usize], "StateName"), Value::Null)),
        ]);
        match out.get(&name) {
            None => {
                out.insert(name, m);
            }
            Some(had) if *had != m => {
                return fail(format!("animgraph: {name0} has two machines {name} with different flags"));
            }
            Some(_) => {}
        }
    }
    Ok(Value::Obj(out))
}

/// A slot set: its names sorted.
type SlotSet = Vec<String>;

fn union(a: &SlotSet, b: &SlotSet) -> SlotSet {
    let mut u: Vec<String> = a.iter().chain(b).cloned().collect::<BTreeSet<_>>().into_iter().collect();
    u.sort();
    u
}

/// The sets that hold no other, by (size, names).
fn minimal(sets: Vec<SlotSet>) -> Vec<SlotSet> {
    let mut uniq: IndexMap<String, SlotSet> = IndexMap::new();
    for st in sets {
        uniq.insert(st.join("\0"), st);
    }
    let mut sorted: Vec<SlotSet> = uniq.into_values().collect();
    sorted.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| cmp_list(a, b)));
    let mut keep: Vec<SlotSet> = Vec::new();
    for st in sorted {
        if !keep.iter().any(|k| k.iter().all(|x| st.contains(x))) {
            keep.push(st);
        }
    }
    keep
}

/// The graph's nodes as the analysis walks them.
struct Graph<'b> {
    bp: &'b AnimBp,
    nodes: Vec<&'b ChildProp>,
    kinds: Vec<&'b str>,
    vals: Vec<Value>,
}

impl Graph<'_> {
    fn slot_name(&self, n: usize) -> Value {
        or(at(&self.vals[n], "SlotName"), s("DefaultSlot"))
    }

    fn always(&self, n: usize) -> bool {
        b_or(at(&self.vals[n], "bAlwaysUpdateSourcePose"), false)
    }
}

/// {class, nodes: [{node, slot, always}], stages: [[slot, ...], ...], machines, graph} of the AnimBP. `node_kinds`:
/// the character is opted into the graph's 'seq' / 'layered' / 'machine' kinds (FighterPort anim_node_kinds).
pub fn slot_holds(bp: &AnimBp, node_kinds: bool) -> Result<Props, KindError> {
    let name0 = bp.name();
    let nodes = anim_node_props(&bp.native);
    let kinds: Vec<&str> = nodes.iter().map(|c| struct_of(c)).collect();
    let vals: Vec<Value> = nodes.iter().map(|c| bp.own(prop_name(c))).collect();
    let ins: Vec<Vec<usize>> = vals
        .iter()
        .map(|v| {
            let mut o = Vec::new();
            links(v, &mut o);
            o
        })
        .collect();
    let roots: Vec<usize> = (0..kinds.len())
        .filter(|&i| kinds[i] == "AnimNode_Root" && s_or(at(&vals[i], "Name"), "AnimGraph") == "AnimGraph")
        .collect();
    if roots.len() != 1 {
        return fail(format!("animgraph: {name0} has {} Root nodes named AnimGraph, 1 expected", roots.len()));
    }
    let reach = |start: usize| -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![start];
        while let Some(n) = todo.pop() {
            if !seen.insert(n) {
                continue;
            }
            if let Some(i) = ins.get(n) {
                todo.extend(i.iter().copied());
            }
        }
        seen
    };
    let linked: BTreeSet<usize> = (0..kinds.len())
        .filter(|&i| kinds[i] == "AnimNode_LinkedAnimLayer" || kinds[i] == "AnimNode_LinkedAnimGraph")
        .collect();
    // A state's graph ends in a StateResult node; the machine it belongs to is named by the class's baked machine
    // (BakedStateMachines[m].States[s].StateRootNodeIndex, an index from the END of the node list: the engine's
    // property order is the reverse of the list's).
    let machines = arr_of(bp.cls.get("BakedStateMachines"));
    let mut targets = linked.clone();
    for i in 0..kinds.len() {
        if kinds[i] != "AnimNode_StateMachine" {
            continue;
        }
        let idx = n_or(at(&vals[i], "StateMachineIndexInClass"), -1.0);
        if !(idx >= 0.0 && (idx as usize) < machines.len()) {
            continue;
        }
        for st in arr_of(at(&machines[idx as usize], "States")) {
            let r = n_or(at(st, "StateRootNodeIndex"), -1.0);
            if r >= 0.0
                && (r as usize) < nodes.len()
                && reach(nodes.len() - 1 - r as usize).iter().any(|x| linked.contains(x))
            {
                targets.insert(i);
            }
        }
    }
    let main = reach(roots[0]);
    targets.retain(|t| main.contains(t));
    let g = Graph { bp, nodes, kinds, vals };
    let slots: Vec<Value> = main
        .iter()
        .filter(|&&i| g.kinds[i] == "AnimNode_Slot")
        .map(|&i| {
            obj([("node", s(prop_name(g.nodes[i]))), ("slot", g.slot_name(i)), ("always", Value::Bool(g.always(i)))])
        })
        .collect();
    let mut memo: HashMap<usize, Option<Vec<SlotSet>>> = HashMap::new();
    let c = cuts(&g, &ins, &targets, roots[0], &mut memo)?;
    let stages: Vec<Value> =
        c.unwrap_or_default().into_iter().map(|st| Value::Arr(st.into_iter().map(Value::Str).collect())).collect();
    let mut out = Props::new();
    out.insert("class".into(), s(name0));
    out.insert("nodes".into(), Value::Arr(slots));
    out.insert("stages".into(), Value::Arr(stages));
    out.insert("machines".into(), state_machines(bp)?);
    out.insert("graph".into(), slot_graph(&g, roots[0], &targets, node_kinds)?);
    Ok(out)
}

fn cuts(
    g: &Graph<'_>,
    ins: &[Vec<usize>],
    targets: &BTreeSet<usize>,
    n: usize,
    memo: &mut HashMap<usize, Option<Vec<SlotSet>>>,
) -> Result<Option<Vec<SlotSet>>, KindError> {
    if let Some(m) = memo.get(&n) {
        return Ok(m.clone());
    }
    memo.insert(n, None); // a loop in the links (none in a valid graph) reaches nothing new
    let res = if targets.contains(&n) {
        Some(Vec::new())
    } else {
        let mut below = Vec::new();
        for &i in &ins[n] {
            if let Some(c) = cuts(g, ins, targets, i, memo)? {
                below.push(c);
            }
        }
        if below.is_empty() {
            None
        } else {
            let mut res: Vec<SlotSet> = vec![Vec::new()];
            for c in &below {
                res = res.iter().flat_map(|a| c.iter().map(|b| union(a, b))).collect();
                if res.len() > 4096 {
                    return fail(format!("animgraph: {}: too many slot sets", g.bp.name()));
                }
            }
            res = minimal(res);
            if g.kinds[n] == "AnimNode_Slot" && !g.always(n) {
                let name = str_of(Some(&g.slot_name(n))).unwrap_or_default().to_owned();
                res.push(vec![name]);
                res = minimal(res);
            }
            Some(res)
        }
    };
    memo.insert(n, res.clone());
    Ok(res)
}

// The node kinds with one pose input that slot_graph passes through: they change the bones of the pose, not which
// track it comes from (the kinds the roster's AnimBPs hold on the way from the root to the locomotion). A one-input
// kind not named here is written as 'other', which the game refuses, so a new kind is read before it is passed.
const PASS_THROUGH: &[&str] = &[
    "AnimNode_Root",
    "AnimNode_ModifyBone",
    "AnimNode_ConvertComponentToLocalSpace",
    "AnimNode_ConvertLocalToComponentSpace",
    "WAnimationMirrorNode",
    "AnimNode_AnimDynamics",
    "AnimNode_SaveCachedPose",
    "AnimNode_UseCachedPose",
    "AnimNode_Trail",
    "AnimNode_SpringBone",
    "AnimNode_Constraint",
    "AnimNode_LegIK",
];
// Passed through as well, only for a character opted into the node kinds: another character's graph keeps them as
// 'other', as it was built before them.
const PASS_THROUGH_KINDS: &[&str] = &["AnimNode_BoneDrivenController", "AnimNode_CopyBone"];

/// The slot graph's walk: the node ids handed out and the nodes written.
struct SlotWalk<'g, 'b> {
    g: &'g Graph<'b>,
    targets: &'g BTreeSet<usize>,
    node_kinds: bool,
    bind: Bindings,
    named: Vec<Vec<(String, usize)>>,
    ids: HashMap<usize, usize>,
    out: Vec<Value>,
}

impl SlotWalk<'_, '_> {
    fn follow(&self, mut n: usize) -> usize {
        loop {
            if self.targets.contains(&n) {
                return n;
            }
            let k = self.g.kinds[n];
            let pass = PASS_THROUGH.contains(&k) || (self.node_kinds && PASS_THROUGH_KINDS.contains(&k));
            if !pass || self.named[n].len() != 1 {
                return n;
            }
            n = self.named[n][0].1;
        }
    }

    /// The id of the node a link at property path `p` of node `n` leads to.
    fn at(&mut self, n: usize, p: &str) -> Result<Value, KindError> {
        let Some(&(_, l)) = self.named[n].iter().rev().find(|(q, _)| q == p) else {
            return fail(format!(
                "animgraph: {}: {} has no pose link {p}",
                self.g.bp.name(),
                prop_name(self.g.nodes[n])
            ));
        };
        Ok(num(self.node_id(l)? as f64))
    }

    fn node_id(&mut self, n0: usize) -> Result<usize, KindError> {
        let n = self.follow(n0);
        if let Some(&id) = self.ids.get(&n) {
            return Ok(id);
        }
        let id = self.out.len();
        self.ids.insert(n, id);
        self.out.push(Value::Null);
        let g = self.g;
        let k = g.kinds[n];
        let v = &g.vals[n];
        let name = prop_name(g.nodes[n]).to_owned();
        let mut e = Props::new();
        if self.targets.contains(&n) {
            put(&mut e, "k", s("src"));
            put(&mut e, "node", s(&name));
            put(&mut e, "kind", s(k));
        } else if k == "AnimNode_Slot" {
            put(&mut e, "k", s("slot"));
            put(&mut e, "node", s(&name));
            put(&mut e, "slot", g.slot_name(n));
            let src = self.at(n, "Source")?;
            put(&mut e, "src", src);
        } else if k == "AnimNode_TwoWayBlend" {
            put(&mut e, "k", s("two"));
            put(&mut e, "node", s(&name));
            put(&mut e, "in", bound(&self.bind, &name, "Alpha"));
            put(&mut e, "type", s(tail(s_or(at(v, "AlphaInputType"), "EAnimAlphaInputType::Float"))));
            put(&mut e, "alpha", or(at(v, "Alpha"), num(0.0)));
            let a = self.at(n, "A")?;
            put(&mut e, "a", a);
            let b = self.at(n, "B")?;
            put(&mut e, "b", b);
            let sb = or(at(v, "AlphaScaleBias"), Value::Obj(Props::new()));
            let sbc = or(at(v, "AlphaScaleBiasClamp"), Value::Obj(Props::new()));
            // FInputScaleBiasClamp::ApplyTo 14288b2b0 applies the clamp struct's own Scale / Bias whatever its flags.
            if n_or(at(&sb, "Scale"), 1.0) != 1.0
                || n_or(at(&sb, "Bias"), 0.0) != 0.0
                || n_or(at(&sbc, "Scale"), 1.0) != 1.0
                || n_or(at(&sbc, "Bias"), 0.0) != 0.0
                || ["bMapRange", "bClampResult", "bInterpResult"].iter().any(|f| truthy(at(&sbc, f)))
            {
                put(&mut e, "scaleBias", obj([("scaleBias", sb), ("clamp", sbc)]));
            }
        } else if k == "AnimNode_BlendListByBool" {
            put(&mut e, "k", s("bool"));
            put(&mut e, "node", s(&name));
            put(&mut e, "in", bound(&self.bind, &name, "bActiveValue"));
            put(&mut e, "value", Value::Bool(b_or(at(v, "bActiveValue"), false)));
            put(&mut e, "blend", or_empty(at(v, "BlendTime")));
            let mut poses = Vec::new();
            for i in 0..arr_of(at(v, "BlendPose")).len() {
                poses.push(self.at(n, &format!("BlendPose[{i}]"))?);
            }
            put(&mut e, "poses", Value::Arr(poses));
            // FAnimNode_BlendListBase's other settings, only where not the ones the game's slot blend ports (a
            // standard transition, a linear FAlphaBlend, no child reset on activation, no blend profile or custom
            // curve): the game refuses a walked list that has them.
            let opts = [
                ("TransitionType", s(tail(s_or(at(v, "TransitionType"), "EBlendListTransitionType::StandardBlend")))),
                ("BlendType", s(tail(s_or(at(v, "BlendType"), "EAlphaBlendOption::Linear")))),
                ("bResetChildOnActivation", Value::Bool(b_or(at(v, "bResetChildOnActivation"), false))),
                ("BlendProfile", or(at(v, "BlendProfile"), Value::Null)),
                ("CustomBlendCurve", or(at(v, "CustomBlendCurve"), Value::Null)),
            ];
            let std = [s("StandardBlend"), s("Linear"), Value::Bool(false), Value::Null, Value::Null];
            let diff: Props = opts
                .into_iter()
                .zip(std)
                .filter(|((_, a), b)| !strict_eq(a, b))
                .map(|((k, a), _)| (k.to_owned(), a))
                .collect();
            if !diff.is_empty() {
                put(&mut e, "opts", Value::Obj(diff));
            }
        } else if k == "AnimNode_SequencePlayer" && self.node_kinds {
            let seq = str_of(at(v, "Sequence"));
            put(&mut e, "k", s("seq"));
            put(&mut e, "node", s(&name));
            put(&mut e, "anim", seq.map_or(Value::Null, |q| s(short_name(q))));
            put(&mut e, "rate", or(at(v, "PlayRate"), num(1.0)));
            put(&mut e, "start", or(at(v, "StartPosition"), num(0.0)));
            put(&mut e, "loop", Value::Bool(b_or(at(v, "bLoopAnimation"), true)));
            put(&mut e, "player", num((g.nodes.len() - 1 - n) as f64));
            // what the game does not port (its slot blend refuses a walked player with it): a bound pin, a play
            // rate basis or scale / bias / clamp, a sync group
            let prc = or(at(v, "PlayRateScaleBiasClamp"), Value::Obj(Props::new()));
            let mut opts = Props::new();
            let mut props: Vec<String> = self
                .bind
                .keys()
                .filter_map(|key| {
                    let (nn, p) = key.split_once('\0')?;
                    (nn == name).then(|| p.to_owned())
                })
                .collect();
            if !props.is_empty() {
                props.sort();
                let list = props.into_iter().map(|p| s(if p.is_empty() { "function".to_owned() } else { p })).collect();
                opts.insert("bound".into(), Value::Arr(list));
            }
            if n_or(at(v, "PlayRateBasis"), 1.0) != 1.0 {
                opts.insert("PlayRateBasis".into(), or(at(v, "PlayRateBasis"), Value::Null));
            }
            if n_or(at(&prc, "Scale"), 1.0) != 1.0
                || n_or(at(&prc, "Bias"), 0.0) != 0.0
                || ["bMapRange", "bClampResult", "bInterpResult"].iter().any(|f| truthy(at(&prc, f)))
            {
                opts.insert("PlayRateScaleBiasClamp".into(), prc);
            }
            if let Some(gn) = some(at(v, "GroupName"))
                && str_of(Some(gn)) != Some("None")
            {
                opts.insert("GroupName".into(), gn.clone());
            }
            if truthy(at(v, "bIgnoreForRelevancyTest")) {
                put(&mut e, "ignore", Value::Bool(true));
            }
            if !opts.is_empty() {
                put(&mut e, "opts", Value::Obj(opts));
            }
        } else if k == "AnimNode_LayeredBoneBlend" && self.node_kinds {
            // (the blends' ids before the base's)
            let mut blends = Vec::new();
            for i in 0..arr_of(at(v, "BlendPoses")).len() {
                blends.push(self.at(n, &format!("BlendPoses[{i}]"))?);
            }
            put(&mut e, "k", s("layered"));
            put(&mut e, "node", s(&name));
            let base = self.at(n, "BasePose")?;
            put(&mut e, "base", base);
            put(&mut e, "blends", Value::Arr(blends));
            put(&mut e, "in", bound(&self.bind, &name, "BlendWeights"));
            put(&mut e, "weights", or_empty(at(v, "BlendWeights")));
            let filters = arr_of(at(v, "LayerSetup"))
                .iter()
                .map(|l| {
                    Value::Arr(
                        arr_of(at(l, "BranchFilters"))
                            .iter()
                            .map(|f| {
                                obj([
                                    ("bone", or(at(f, "BoneName"), Value::Null)),
                                    ("depth", or(at(f, "BlendDepth"), num(0.0))),
                                ])
                            })
                            .collect(),
                    )
                })
                .collect();
            put(&mut e, "filters", Value::Arr(filters));
            put(&mut e, "meshRot", Value::Bool(b_or(at(v, "bMeshSpaceRotationBlend"), false)));
            put(&mut e, "meshScale", Value::Bool(b_or(at(v, "bMeshSpaceScaleBlend"), false)));
        } else if k == "AnimNode_StateMachine" && self.named[n].is_empty() && self.node_kinds {
            e = self.machine(n)?;
        } else {
            put(&mut e, "k", s("other"));
            put(&mut e, "node", s(&name));
            put(&mut e, "kind", s(k));
            let mut poses = Vec::new();
            for (_, l) in self.named[n].clone() {
                poses.push(num(self.node_id(l)? as f64));
            }
            put(&mut e, "poses", Value::Arr(poses));
        }
        self.out[id] = Value::Obj(e);
        Ok(id)
    }

    /// A state machine node that holds no locomotion target, as a pose source of the slot graph. Node indexes of
    /// the baked machine count from the END of the class's node list (the engine's property order), as
    /// StateRootNodeIndex does in slot_holds.
    fn machine(&mut self, n: usize) -> Result<Props, KindError> {
        let g = self.g;
        let name0 = g.bp.name();
        let v = &g.vals[n];
        let baked = arr_of(g.bp.cls.get("BakedStateMachines"));
        let idx = n_or(at(v, "StateMachineIndexInClass"), -1.0);
        if !(idx >= 0.0 && (idx as usize) < baked.len()) {
            return fail(format!(
                "animgraph: {name0}.{}: a state machine without its baked machine",
                prop_name(g.nodes[n])
            ));
        }
        let b = &baked[idx as usize];
        let mname = str_of(at(b, "MachineName")).unwrap_or_default();
        let last = g.nodes.len() - 1;
        let bind = self.bind.clone();
        let result_in = |i: &Value, what: &str| -> Result<(Value, Value), KindError> {
            let i = n_or(Some(i), f64::NAN);
            if !(i >= 0.0 && i <= last as f64) || g.kinds[last - i as usize] != "AnimNode_TransitionResult" {
                return fail(format!("animgraph: {name0} machine {mname}: {what} {i} is not a TransitionResult node"));
            }
            let r = last - i as usize;
            let rn = prop_name(g.nodes[r]);
            Ok((
                bound(&bind, rn, "bCanEnterTransition"),
                Value::Bool(b_or(at(&g.vals[r], "bCanEnterTransition"), false)),
            ))
        };
        let mut trans: Vec<Props> = Vec::new();
        for t in arr_of(at(b, "Transitions")) {
            let mut e = Props::new();
            e.insert("from".into(), or(at(t, "PreviousState"), Value::Null));
            e.insert("to".into(), or(at(t, "NextState"), Value::Null));
            e.insert("xfade".into(), or(at(t, "CrossfadeDuration"), num(0.0)));
            e.insert("blend".into(), enum_tail(or(at(t, "BlendMode"), s("EAlphaBlendOption::HermiteCubic"))));
            let mut opts = Props::new();
            if tail(s_or(at(t, "LogicType"), "ETransitionLogicType::TLT_StandardBlend")) != "TLT_StandardBlend" {
                opts.insert("LogicType".into(), enum_tail(or(at(t, "LogicType"), s(""))));
            }
            for f in ["BlendProfile", "CustomCurve"] {
                if truthy(at(t, f)) {
                    opts.insert(f.into(), at(t, f).cloned().unwrap_or(Value::Null));
                }
            }
            if !opts.is_empty() {
                e.insert("opts".into(), Value::Obj(opts));
            }
            trans.push(e);
        }
        let mut states = Vec::new();
        for st0 in arr_of(at(b, "States")) {
            let sname = or(at(st0, "StateName"), Value::Null);
            let mut st = Props::new();
            st.insert("name".into(), sname.clone());
            st.insert("players".into(), or_empty(at(st0, "PlayerNodeIndices")));
            st.insert("alwaysReset".into(), Value::Bool(b_or(at(st0, "bAlwaysResetOnEntry"), false)));
            st.insert("out".into(), Value::Arr(Vec::new()));
            if truthy(at(st0, "bIsAConduit")) {
                st.insert("pose".into(), Value::Null);
                if n_or(at(st0, "EntryRuleNodeIndex"), -1.0) >= 0.0 {
                    let (i, val) = result_in(at(st0, "EntryRuleNodeIndex").unwrap_or(&Value::Null), "entry rule")?;
                    st.insert("entryIn".into(), i);
                    st.insert("entryValue".into(), val);
                }
            } else {
                let r = n_or(at(st0, "StateRootNodeIndex"), -1.0);
                if !(r >= 0.0 && r <= last as f64) || g.kinds[last - r as usize] != "AnimNode_StateResult" {
                    return fail(format!(
                        "animgraph: {name0} machine {mname} state {}: no StateResult node",
                        str_of(Some(&sname)).unwrap_or_default()
                    ));
                }
                let mut res = Vec::new();
                links(&g.vals[last - r as usize], &mut res);
                let pose = match res.first() {
                    Some(&l) => num(self.node_id(l)? as f64),
                    None => Value::Null,
                };
                st.insert("pose".into(), pose);
            }
            let mut outs = Vec::new();
            for bt in arr_of(at(st0, "Transitions")) {
                let ti = n_or(at(bt, "TransitionIndex"), f64::NAN);
                if !(ti >= 0.0 && (ti as usize) < trans.len()) {
                    return fail(format!("animgraph: {name0} machine {mname}: transition {ti} out of range"));
                }
                let t = &mut trans[ti as usize];
                if !t.contains_key("in") {
                    let (i, val) = if n_or(at(bt, "CanTakeDelegateIndex"), -1.0) >= 0.0 {
                        result_in(at(bt, "CanTakeDelegateIndex").unwrap_or(&Value::Null), "transition rule")?
                    } else {
                        (Value::Null, Value::Bool(false))
                    };
                    t.insert("in".into(), i);
                    t.insert("value".into(), val);
                    t.insert("want".into(), Value::Bool(b_or(at(bt, "bDesiredTransitionReturnValue"), true)));
                    t.insert("auto".into(), Value::Bool(b_or(at(bt, "bAutomaticRemainingTimeRule"), false)));
                    if n_or(at(bt, "CustomResultNodeIndex"), -1.0) >= 0.0 {
                        let o = t.entry("opts".to_owned()).or_insert_with(|| Value::Obj(Props::new()));
                        if let Value::Obj(o) = o {
                            o.insert("CustomResultNodeIndex".into(), or(at(bt, "CustomResultNodeIndex"), Value::Null));
                        }
                    }
                }
                outs.push(num(ti));
            }
            st.insert("out".into(), Value::Arr(outs));
            states.push(Value::Obj(st));
        }
        let mut m = Props::new();
        m.insert("k".into(), s("machine"));
        m.insert("node".into(), s(prop_name(g.nodes[n])));
        m.insert("index".into(), num((last - n) as f64));
        m.insert("machine".into(), or(at(b, "MachineName"), Value::Null));
        m.insert("entry".into(), or(at(b, "InitialState"), num(0.0)));
        m.insert("max".into(), or(at(v, "MaxTransitionsPerFrame"), num(3.0)));
        m.insert("reinit".into(), Value::Bool(b_or(at(v, "bReinitializeOnBecomingRelevant"), false)));
        m.insert("skipFirst".into(), Value::Bool(b_or(at(v, "bSkipFirstUpdateTransition"), false)));
        m.insert("states".into(), Value::Arr(states));
        m.insert("trans".into(), Value::Arr(trans.into_iter().map(Value::Obj).collect()));
        Ok(m)
    }
}

/// JS `!==`'s complement on the values the walk compares (a string, a boolean, null; an object only to itself).
fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Obj(_) | Value::Arr(_), _) | (_, Value::Obj(_) | Value::Arr(_)) => false,
        _ => a == b,
    }
}

fn put(e: &mut Props, key: &str, x: Value) {
    e.insert(key.to_owned(), x);
}

fn enum_tail(x: Value) -> Value {
    match x {
        Value::Str(t) => s(tail(&t)),
        x => x,
    }
}

/// The pose links from the Root named AnimGraph down to the locomotion, as far as they choose which montage track is
/// shown: what the sim needs to pick the pose of a montage with no DefaultSlot or with two slots under a blend.
/// {root: id, nodes: [...]} (and `cached` with `node_kinds`). The node shapes are slot, two, bool, src, seq, layered,
/// machine, other.
fn slot_graph(g: &Graph<'_>, root: usize, targets: &BTreeSet<usize>, node_kinds: bool) -> Result<Value, KindError> {
    let name0 = g.bp.name();
    let mut w = SlotWalk {
        g,
        targets,
        node_kinds,
        bind: bindings(g.bp)?,
        named: g.vals.iter().map(named_links).collect(),
        ids: HashMap::new(),
        out: Vec::new(),
    };
    let r = w.node_id(root)?;
    if !node_kinds {
        return Ok(obj([("root", num(r as f64)), ("nodes", Value::Arr(w.out))]));
    }
    // The SaveCachedPose nodes' sources the graph reaches, in the AnimGraph's OrderedSavedPoseNodeIndices order (baked
    // indexes, counted from the end): FAnimNode_SaveCachedPose updates its source once after the root graph's update,
    // with the heaviest context, in that order (FAnimNode_SaveCachedPose Update 142867de0).
    let mut cached: Vec<usize> = Vec::new();
    for entry in arr_of(g.bp.cls.get("OrderedSavedPoseIndicesMap")) {
        let (key, value) = match entry {
            Value::Arr(p) => (p.first(), p.get(1)),
            e => (at(e, "Key"), at(e, "Value")),
        };
        if str_of(key) != Some("AnimGraph") {
            continue;
        }
        for o in arr_of(value.and_then(|v| at(v, "OrderedSavedPoseNodeIndices"))) {
            let o = n_or(Some(o), f64::NAN);
            let n = g.nodes.len() as f64 - 1.0 - o;
            if !(n >= 0.0 && (n as usize) < g.nodes.len()) || g.kinds[n as usize] != "AnimNode_SaveCachedPose" {
                return fail(format!("animgraph: {name0}: OrderedSavedPoseNodeIndices {o} is no SaveCachedPose"));
            }
            let n = n as usize;
            if w.named[n].len() != 1 {
                return fail(format!(
                    "animgraph: {name0}: {} has {} pose links",
                    prop_name(g.nodes[n]),
                    w.named[n].len()
                ));
            }
            let src = w.follow(w.named[n][0].1);
            if let Some(&id) = w.ids.get(&src)
                && !cached.contains(&id)
            {
                cached.push(id);
            }
        }
    }
    let cached = cached.into_iter().map(|c| num(c as f64)).collect();
    Ok(obj([("root", num(r as f64)), ("nodes", Value::Arr(w.out)), ("cached", Value::Arr(cached))]))
}

// The skeletal control nodes the game enters by struct name and the layered blends, with their class default values,
// for a character's pose code to make them from the data. The pose links and the cooked per-bone weights are left out
// (the game builds a layered blend's mask from its LayerSetup).
const ANIM_NODE_KINDS: &[&str] = &[
    "AnimNode_AnimDynamics",
    "AnimNode_BoneDrivenController",
    "AnimNode_CopyBone",
    "AnimNode_Constraint",
    "AnimNode_LayeredBoneBlend",
    "AnimNode_SpringBone",
    "AnimNode_LegIK",
    "AnimNode_Trail",
];

/// {node name: {type: struct name, props: the class default value, in: {property or '': input}}} of the AnimBP's nodes
/// of ANIM_NODE_KINDS; `in` as slot_graph's (a property access copy, or the node's name for a bound function under '').
pub fn anim_nodes(bp: &AnimBp) -> Result<Value, KindError> {
    let bind = bindings(bp)?;
    let mut out = Props::new();
    for c in child_props(&bp.native) {
        let k = struct_of(c);
        if !k.contains("AnimNode_") || !ANIM_NODE_KINDS.contains(&k) {
            continue;
        }
        let v = bp.own(prop_name(c));
        let mut p = Props::new();
        for (key, x) in v.as_obj().into_iter().flatten() {
            if key == "PerBoneBlendWeights" || key == "ActualAlpha" || key == "BlendPoses" || is_link(x) {
                continue;
            }
            p.insert(key.clone(), x.clone());
        }
        let mut ins = Props::new();
        for (key, src) in &bind {
            if let Some((nn, prop)) = key.split_once('\0')
                && nn == prop_name(c)
            {
                ins.insert(prop.to_owned(), s(src.clone()));
            }
        }
        out.insert(prop_name(c).to_owned(), obj([("type", s(k)), ("props", Value::Obj(p)), ("in", Value::Obj(ins))]));
    }
    Ok(Value::Obj(out))
}
