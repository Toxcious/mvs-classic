//! The match's path rule (`mvs_protocol::path`) on vectors: whether a match is measured and who referees it.

use mvs_protocol::path::{PathChoice, PathInput, choose, should_measure};

/// One case: by human index the round trips to the relay, who is on Epic, the round trips each would report to each
/// other one (ms), whether it is measured and the choice.
struct PathVector {
    name: &'static str,
    relay: Vec<Option<f64>>,
    epic: Vec<bool>,
    links: Vec<Vec<Option<f64>>>,
    measure: bool,
    want: PathChoice,
}

/// A symmetric link table from the pairs' round trips (None: the link did not stand).
fn sym(n: usize, pairs: &[(usize, usize, Option<f64>)]) -> Vec<Vec<Option<f64>>> {
    let mut l = vec![vec![None; n]; n];
    for &(a, b, r) in pairs {
        l[a][b] = r;
        l[b][a] = r;
    }
    l
}

fn vectors() -> Vec<PathVector> {
    use PathChoice::{Player, Relay};
    vec![
        PathVector {
            name: "near players are not measured",
            relay: vec![Some(30.0), Some(60.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(10.0))]),
            measure: false,
            want: Relay,
        },
        PathVector {
            name: "a far pair with a short Epic way takes the first player",
            relay: vec![Some(200.0), Some(180.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(40.0))]),
            measure: true,
            want: Player(0),
        },
        PathVector {
            name: "one far player is enough to measure",
            relay: vec![Some(20.0), Some(150.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(60.0))]),
            measure: true,
            want: Player(0),
        },
        PathVector {
            name: "a link that did not stand keeps the relay",
            relay: vec![Some(200.0), Some(200.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, None)]),
            measure: true,
            want: Relay,
        },
        PathVector {
            name: "a human without Epic is not measured",
            relay: vec![Some(200.0), Some(200.0)],
            epic: vec![true, false],
            links: sym(2, &[(0, 1, Some(20.0))]),
            measure: false,
            want: Relay,
        },
        PathVector {
            name: "a gain under 20 ms keeps the relay",
            relay: vec![Some(100.0), Some(100.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(161.0))]),
            measure: true,
            want: Relay,
        },
        PathVector {
            name: "a gain of 20 ms exactly takes the player",
            relay: vec![Some(100.0), Some(100.0)],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(160.0))]),
            measure: true,
            want: Player(0),
        },
        PathVector {
            name: "a relay round trip not known: not measured, the relay",
            relay: vec![Some(200.0), None],
            epic: vec![true, true],
            links: sym(2, &[(0, 1, Some(20.0))]),
            measure: false,
            want: Relay,
        },
        PathVector {
            name: "a pair's round trip is the larger report",
            relay: vec![Some(100.0), Some(100.0)],
            epic: vec![true, true],
            links: vec![vec![None, Some(10.0)], vec![Some(170.0), None]],
            measure: true,
            want: Relay,
        },
        PathVector {
            name: "a missing report is a failed link",
            relay: vec![Some(200.0), Some(200.0)],
            epic: vec![true, true],
            links: vec![vec![None, Some(30.0)], vec![]],
            measure: true,
            want: Relay,
        },
        PathVector {
            name: "three humans: the one between the others",
            relay: vec![Some(250.0), Some(250.0), Some(250.0)],
            epic: vec![true, true, true],
            links: sym(3, &[(0, 1, Some(40.0)), (1, 2, Some(40.0)), (0, 2, Some(300.0))]),
            measure: true,
            want: Player(1),
        },
        PathVector {
            name: "three humans tied: the lowest index",
            relay: vec![Some(250.0), Some(250.0), Some(250.0)],
            epic: vec![true, true, true],
            links: sym(3, &[(0, 1, Some(40.0)), (1, 2, Some(40.0)), (0, 2, Some(40.0))]),
            measure: true,
            want: Player(0),
        },
        PathVector {
            name: "three humans: a candidate needs every link to it, not the others' to each other",
            relay: vec![Some(250.0), Some(250.0), Some(250.0)],
            epic: vec![true, true, true],
            links: sym(3, &[(0, 1, Some(40.0)), (1, 2, Some(40.0)), (0, 2, None)]),
            measure: true,
            want: Player(1),
        },
        PathVector {
            name: "four humans: the best star",
            relay: vec![Some(300.0), Some(300.0), Some(300.0), Some(300.0)],
            epic: vec![true, true, true, true],
            links: sym(
                4,
                &[
                    (0, 1, Some(100.0)),
                    (0, 2, Some(100.0)),
                    (0, 3, Some(100.0)),
                    (1, 2, Some(150.0)),
                    (1, 3, Some(20.0)),
                    (2, 3, Some(60.0)),
                ],
            ),
            measure: true,
            // (worst through 0: 100, 1: 150 + 20 / 2 = 85 .. (150 + 100) / 2 = 125, 3: (100 + 60) / 2 = 80)
            want: Player(3),
        },
        PathVector {
            name: "four humans, one link missing to every candidate but one",
            relay: vec![Some(300.0), Some(300.0), Some(300.0), Some(300.0)],
            epic: vec![true, true, true, true],
            links: sym(4, &[(0, 1, Some(50.0)), (0, 2, Some(50.0)), (0, 3, Some(50.0))]),
            measure: true,
            want: Player(0),
        },
    ]
}

#[test]
fn the_vectors_hold() {
    for v in vectors() {
        assert_eq!(should_measure(&v.relay, &v.epic), v.measure, "{}: measure", v.name);
        // (a match not measured is chosen with no links, as the core does)
        let links = if v.measure { v.links.clone() } else { Vec::new() };
        let o = choose(&PathInput { relay_rtt: v.relay.clone(), links });
        assert_eq!(o.choice, v.want, "{}: choice", v.name);
        if let PathChoice::Player(r) = o.choice {
            assert_eq!(o.rtt_to_referee[r], Some(0.0), "{}: the referee's own", v.name);
        }
    }
}
