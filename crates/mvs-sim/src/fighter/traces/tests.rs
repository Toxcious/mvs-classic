use super::*;
use mvs_data::value::{Props, Value};

fn obj(kv: &[(&str, Value)]) -> Value {
    Value::Obj(kv.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect())
}

fn s(v: &str) -> Value {
    Value::Str(v.to_owned())
}

/// A template whose body answers HurtBox with Overlap and Pawn with Ignore, and names no other channel.
fn template() -> Props {
    let arr = Value::Arr(vec![
        obj(&[("Channel", s("HurtBox")), ("Response", s("ECR_Overlap"))]),
        obj(&[("Channel", s("Pawn")), ("Response", s("ECR_Ignore"))]),
        obj(&[("Channel", s("Item"))]),
    ]);
    let b = obj(&[("CollisionResponses", obj(&[("ResponseArray", arr)]))]);
    [("BodyInstance".to_owned(), b)].into_iter().collect()
}

#[test]
fn response_to_reads_the_array_then_the_channel_default() {
    let t = template();
    assert_eq!(response_to(Some(&t), crate::real(OBJ_HURT_BOX)), name!("ECR_Overlap"));
    assert_eq!(response_to(Some(&t), crate::real(OBJ_PAWN)), name!("ECR_Ignore"));
    // an entry without a Response is Block
    assert_eq!(response_to(Some(&t), crate::real(OBJ_ITEM)), name!("ECR_Block"));
    // the engine channels' default Block, the project's per DefaultEngine.ini
    assert_eq!(response_to(Some(&t), crate::real(OBJ_WORLD_STATIC)), name!("ECR_Block"));
    assert_eq!(response_to(None, crate::real(OBJ_ITEM)), name!("ECR_Overlap"));
    assert_eq!(response_to(None, crate::real(OBJ_KNOCK_BACK)), name!("ECR_Ignore"));
    assert_eq!(response_to(None, crate::real(OBJ_PLATFORM)), name!("ECR_Block"));
    // an index past the table: no channel, Block
    assert_eq!(response_to(None, 30.0), name!("ECR_Block"));
}

#[test]
fn object_channels_and_profile_blocks() {
    assert_eq!(ecc("ECC_WorldStatic"), Some(0));
    assert_eq!(ecc("ECC_GameTraceChannel7"), Some(OBJ_PLATFORM));
    assert_eq!(ecc("ECC_GameTraceChannel17"), Some(OBJ_HURT_BOX));
    assert_eq!(ecc("ECC_GameTraceChannel2"), None);
    assert_eq!(profile_blocks("Floor"), 0b11_1100);
    assert_eq!(profile_blocks("Platform"), 1 << TRACE_PROJECTILE_GROUND);
    assert_eq!(profile_blocks("Pawn"), 0);
    assert_eq!(profile_name("Floor"), name!("Floor"));
    assert_eq!(profile_name("NotAProfile"), Name::NONE);
    assert!(is_shape_template(&[("SphereRadius".to_owned(), Value::Num(3.0))].into_iter().collect()));
    assert!(!is_shape_template(&template()));
}

#[test]
fn the_filter() {
    let me = Some(ActorRef::Fighter(crate::fighter::FighterId::new(0, 0)));
    let ign = [None];
    let q = objects(&[0.0, 18.0], &ign, me, -1);
    assert_eq!(q.mask, 1 | (1 << 18));
    assert!(wants(&q, OBJ_WORLD_STATIC, 0));
    assert!(wants(&q, OBJ_HURT_BOX, 0));
    assert!(!wants(&q, OBJ_PAWN, 0b11_1100));
    // the level's bodies (no actor) are never ignored, the bIgnoreSelf actor is
    assert!(!ignored(&q, None));
    assert!(ignored(&q, me));
    assert!(!not_only(&q, None));
    let c = channel(TRACE_PROJECTILE_GROUND, &[], None);
    assert!(wants(&c, OBJ_PLATFORM, 1 << TRACE_PROJECTILE_GROUND));
    assert!(!wants(&c, OBJ_WORLD_STATIC, 1 << TRACE_WALL_JUMP));
}

fn body(ax: Real, ay: Real, hx: Real, hy: Real, r: Real, kind: i32) -> TraceBody {
    TraceBody {
        ax,
        ay,
        bx: ax,
        by: ay,
        hx,
        hy,
        r,
        z: Real::NAN,
        object_type: kind,
        profile: Name::NONE,
        component: Name::NONE,
        actor: None,
        platform: -1,
        blocks: 0,
        poly: None,
        floor: None,
    }
}

#[test]
fn sweep_all_sorts_by_time_overlaps_ahead() {
    let mut sc = TraceScratch::default();
    // a box ahead, an initial overlap, a box nearer: a sphere of radius 10 from (0, 0) to (100, 0)
    sc.pool.push(body(80.0, 0.0, 5.0, 5.0, 0.0, 0));
    sc.pool.push(body(0.0, 0.0, 0.0, 0.0, 5.0, 1));
    sc.pool.push(body(40.0, 0.0, 5.0, 5.0, 0.0, 2));
    let q = query(0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 10.0, 0.0);
    let mut out = Vec::new();
    sweep_all(&q, &mut sc, &mut out);
    let kinds: Vec<i32> = out.iter().map(|h| h.object_type).collect();
    assert_eq!(kinds, [1, 2, 0]);
    assert!(out[0].initial && out[0].time == 0.0);
    // the sphere's front at 40 - 5 - 10
    assert_eq!(out[1].time, 0.25);
    assert_eq!(out[2].time, 0.65);
}

#[test]
fn box_queries_take_the_adapter_minimum() {
    let q = box_query(0.0, 0.0, 1.0, 0.0, 0.0, 30.0, 0.0);
    assert_eq!((q.qx, q.qy, q.qr, q.qd), (f64::from(MIN_EXTENT), 30.0, 0.0, f64::from(MIN_EXTENT)));
    let line = box_query(0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    assert_eq!((line.qx, line.qy, line.qr, line.qd), (0.0, 0.0, 0.0, 0.0));
    let o = box_overlap_query(3.0, 4.0, 1e-5, 0.0, 0.0);
    assert_eq!((o.dx, o.dy, o.qr), (0.0, 0.0, f64::from(MIN_EXTENT)));
}
