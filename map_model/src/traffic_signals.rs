use crate::{IntersectionID, Map, RoadID, TurnGroup, TurnGroupID, TurnID, TurnPriority, TurnType};
use abstutil::{deserialize_btreemap, retain_btreeset, serialize_btreemap, Timer};
use geom::{Duration, Time};
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ControlTrafficSignal {
    pub id: IntersectionID,
    pub phases: Vec<Phase>,
    pub offset: Duration,

    #[serde(
        serialize_with = "serialize_btreemap",
        deserialize_with = "deserialize_btreemap"
    )]
    pub turn_groups: BTreeMap<TurnGroupID, TurnGroup>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Phase {
    pub protected_groups: BTreeSet<TurnGroupID>,
    pub yield_groups: BTreeSet<TurnGroupID>,
    pub duration: Duration,
}

impl ControlTrafficSignal {
    pub fn new(map: &Map, id: IntersectionID, timer: &mut Timer) -> ControlTrafficSignal {
        let mut policies = ControlTrafficSignal::get_possible_policies(map, id);
        if policies.len() == 1 {
            timer.warn(format!("Falling back to greedy_assignment for {}", id));
        }
        policies.remove(0).1
    }

    pub fn get_possible_policies(
        map: &Map,
        id: IntersectionID,
    ) -> Vec<(String, ControlTrafficSignal)> {
        let mut results = Vec::new();
        if let Some(ts) = ControlTrafficSignal::four_way_four_phase(map, id) {
            results.push(("four-phase".to_string(), ts));
        }
        if let Some(ts) = ControlTrafficSignal::four_way_two_phase(map, id) {
            results.push(("two-phase".to_string(), ts));
        }
        if let Some(ts) = ControlTrafficSignal::three_way(map, id) {
            results.push(("three-phase".to_string(), ts));
        }
        if let Some(ts) = ControlTrafficSignal::degenerate(map, id) {
            results.push(("degenerate (2 roads)".to_string(), ts));
        }
        if let Some(ts) = ControlTrafficSignal::four_oneways(map, id) {
            results.push(("two-phase for 4 one-ways".to_string(), ts));
        }
        if let Some(ts) = ControlTrafficSignal::phase_per_road(map, id) {
            results.push(("phase per road".to_string(), ts));
        }
        results.push((
            "arbitrary assignment".to_string(),
            ControlTrafficSignal::greedy_assignment(map, id),
        ));
        results.push((
            "all walk, then free-for-all yield".to_string(),
            ControlTrafficSignal::all_walk_all_yield(map, id),
        ));
        results
    }

    pub fn current_phase_and_remaining_time(&self, now: Time) -> (usize, &Phase, Duration) {
        let mut cycle_length = Duration::ZERO;
        for p in &self.phases {
            cycle_length += p.duration;
        }

        let mut now_offset = ((now + self.offset) - Time::START_OF_DAY) % cycle_length;
        for (idx, p) in self.phases.iter().enumerate() {
            if now_offset < p.duration {
                return (idx, p, p.duration - now_offset);
            } else {
                now_offset -= p.duration;
            }
        }
        unreachable!()
    }

    pub fn validate(self) -> Result<ControlTrafficSignal, String> {
        // Does the assignment cover the correct set of groups?
        let expected_groups: BTreeSet<TurnGroupID> = self.turn_groups.keys().cloned().collect();
        let mut actual_groups: BTreeSet<TurnGroupID> = BTreeSet::new();
        for phase in &self.phases {
            actual_groups.extend(phase.protected_groups.iter());
            actual_groups.extend(phase.yield_groups.iter());
        }
        if expected_groups != actual_groups {
            return Err(format!(
                "Traffic signal assignment for {} broken. Missing {:?}, contains irrelevant {:?}",
                self.id,
                expected_groups
                    .difference(&actual_groups)
                    .cloned()
                    .collect::<Vec<_>>(),
                actual_groups
                    .difference(&expected_groups)
                    .cloned()
                    .collect::<Vec<_>>()
            ));
        }

        for phase in &self.phases {
            // Do any of the priority groups in one phase conflict?
            for g1 in phase.protected_groups.iter().map(|g| &self.turn_groups[g]) {
                for g2 in phase.protected_groups.iter().map(|g| &self.turn_groups[g]) {
                    if g1.conflicts_with(g2) {
                        return Err(format!(
                            "Traffic signal has conflicting protected groups in one phase:\n{:?}\n\n{:?}",
                            g1, g2
                        ));
                    }
                }
            }

            // Do any of the crosswalks yield?
            for g in phase.yield_groups.iter().map(|g| &self.turn_groups[g]) {
                assert!(g.turn_type != TurnType::Crosswalk);
            }
        }

        Ok(self)
    }

    fn greedy_assignment(map: &Map, intersection: IntersectionID) -> ControlTrafficSignal {
        let turn_groups = TurnGroup::for_i(intersection, map);

        let mut phases = Vec::new();

        // Greedily partition groups into phases that only have protected groups.
        let mut remaining_groups: Vec<TurnGroupID> = turn_groups.keys().cloned().collect();
        let mut current_phase = Phase::new();
        loop {
            let add = remaining_groups
                .iter()
                .position(|&g| current_phase.could_be_protected(g, &turn_groups));
            match add {
                Some(idx) => {
                    current_phase
                        .protected_groups
                        .insert(remaining_groups.remove(idx));
                }
                None => {
                    assert!(!current_phase.protected_groups.is_empty());
                    phases.push(current_phase);
                    current_phase = Phase::new();
                    if remaining_groups.is_empty() {
                        break;
                    }
                }
            }
        }

        expand_all_phases(&mut phases, &turn_groups);

        let ts = ControlTrafficSignal {
            id: intersection,
            phases,
            offset: Duration::ZERO,
            turn_groups,
        };
        // This must succeed
        ts.validate().unwrap()
    }

    fn degenerate(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        if map.get_i(i).roads.len() != 2 {
            return None;
        }

        let mut roads = map.get_i(i).roads.iter();
        let r1 = *roads.next().unwrap();
        let r2 = *roads.next().unwrap();
        // TODO One-ways downtown should also have crosswalks.
        let has_crosswalks = !map.get_r(r1).children_backwards.is_empty()
            || !map.get_r(r2).children_backwards.is_empty();
        let mut phases = vec![vec![(vec![r1, r2], TurnType::Straight, PROTECTED)]];
        if has_crosswalks {
            phases.push(vec![(vec![r1, r2], TurnType::Crosswalk, PROTECTED)]);
        }

        let phases = make_phases(map, i, phases);

        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups: TurnGroup::for_i(i, map),
        };
        ts.validate().ok()
    }

    fn three_way(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        if map.get_i(i).roads.len() != 3 {
            return None;
        }
        let turn_groups = TurnGroup::for_i(i, map);

        // Picture a T intersection. Use turn angles to figure out the "main" two roads.
        let straight = turn_groups
            .values()
            .find(|g| g.turn_type == TurnType::Straight)?;
        let (north, south) = (straight.id.from, straight.id.to);
        let mut roads = map.get_i(i).roads.clone();
        roads.remove(&north);
        roads.remove(&south);
        let east = roads.into_iter().next().unwrap();

        // Two-phase with no protected lefts, right turn on red, turning cars yield to peds
        let phases = make_phases(
            map,
            i,
            vec![
                vec![
                    (vec![north, south], TurnType::Straight, PROTECTED),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Left, YIELD),
                    (vec![east], TurnType::Right, YIELD),
                    (vec![east], TurnType::Crosswalk, PROTECTED),
                ],
                vec![
                    (vec![east], TurnType::Straight, PROTECTED),
                    (vec![east], TurnType::Right, YIELD),
                    (vec![east], TurnType::Left, YIELD),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Crosswalk, PROTECTED),
                ],
            ],
        );

        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups,
        };
        ts.validate().ok()
    }

    fn four_way_four_phase(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        if map.get_i(i).roads.len() != 4 {
            return None;
        }

        // Just to refer to these easily, label with directions. Imagine an axis-aligned four-way.
        let roads = map
            .get_i(i)
            .get_roads_sorted_by_incoming_angle(map.all_roads());
        let (north, west, south, east) = (roads[0], roads[1], roads[2], roads[3]);

        // Four-phase with protected lefts, right turn on red (except for the protected lefts), turning
        // cars yield to peds
        let phases = make_phases(
            map,
            i,
            vec![
                vec![
                    (vec![north, south], TurnType::Straight, PROTECTED),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![east, west], TurnType::Right, YIELD),
                    (vec![east, west], TurnType::Crosswalk, PROTECTED),
                ],
                vec![(vec![north, south], TurnType::Left, PROTECTED)],
                vec![
                    (vec![east, west], TurnType::Straight, PROTECTED),
                    (vec![east, west], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Crosswalk, PROTECTED),
                ],
                vec![(vec![east, west], TurnType::Left, PROTECTED)],
            ],
        );

        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups: TurnGroup::for_i(i, map),
        };
        ts.validate().ok()
    }

    fn four_way_two_phase(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        if map.get_i(i).roads.len() != 4 {
            return None;
        }

        // Just to refer to these easily, label with directions. Imagine an axis-aligned four-way.
        let roads = map
            .get_i(i)
            .get_roads_sorted_by_incoming_angle(map.all_roads());
        let (north, west, south, east) = (roads[0], roads[1], roads[2], roads[3]);

        // Two-phase with no protected lefts, right turn on red, turning cars yielding to peds
        let phases = make_phases(
            map,
            i,
            vec![
                vec![
                    (vec![north, south], TurnType::Straight, PROTECTED),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Left, YIELD),
                    (vec![east, west], TurnType::Right, YIELD),
                    (vec![east, west], TurnType::Crosswalk, PROTECTED),
                ],
                vec![
                    (vec![east, west], TurnType::Straight, PROTECTED),
                    (vec![east, west], TurnType::Right, YIELD),
                    (vec![east, west], TurnType::Left, YIELD),
                    (vec![north, south], TurnType::Right, YIELD),
                    (vec![north, south], TurnType::Crosswalk, PROTECTED),
                ],
            ],
        );

        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups: TurnGroup::for_i(i, map),
        };
        ts.validate().ok()
    }

    fn four_oneways(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        if map.get_i(i).roads.len() != 4 {
            return None;
        }

        let mut incomings = Vec::new();
        for r in &map.get_i(i).roads {
            if !map.get_r(*r).incoming_lanes(i).is_empty() {
                incomings.push(*r);
            }
        }
        if incomings.len() != 2 {
            return None;
        }
        let r1 = incomings[0];
        let r2 = incomings[1];

        // TODO This may not generalize...
        let phases = make_phases(
            map,
            i,
            vec![
                vec![
                    (vec![r1], TurnType::Straight, PROTECTED),
                    (vec![r1], TurnType::Crosswalk, PROTECTED),
                    // TODO Technically, upgrade to protected if there's no opposing crosswalk --
                    // even though it doesn't matter much.
                    (vec![r1], TurnType::Right, YIELD),
                    (vec![r1], TurnType::Left, YIELD),
                    (vec![r1], TurnType::Right, YIELD),
                    // TODO Refactor
                ],
                vec![
                    (vec![r2], TurnType::Straight, PROTECTED),
                    (vec![r2], TurnType::Crosswalk, PROTECTED),
                    // TODO Technically, upgrade to protected if there's no opposing crosswalk --
                    // even though it doesn't matter much.
                    (vec![r2], TurnType::Right, YIELD),
                    (vec![r2], TurnType::Left, YIELD),
                    (vec![r2], TurnType::Right, YIELD),
                ],
            ],
        );

        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups: TurnGroup::for_i(i, map),
        };
        ts.validate().ok()
    }

    fn all_walk_all_yield(map: &Map, i: IntersectionID) -> ControlTrafficSignal {
        let turn_groups = TurnGroup::for_i(i, map);

        let mut all_walk = Phase::new();
        let mut all_yield = Phase::new();

        for group in turn_groups.values() {
            match group.turn_type {
                TurnType::Crosswalk => {
                    all_walk.protected_groups.insert(group.id);
                }
                _ => {
                    all_yield.yield_groups.insert(group.id);
                }
            }
        }

        let ts = ControlTrafficSignal {
            id: i,
            phases: vec![all_walk, all_yield],
            offset: Duration::ZERO,
            turn_groups,
        };
        // This must succeed
        ts.validate().unwrap()
    }

    fn phase_per_road(map: &Map, i: IntersectionID) -> Option<ControlTrafficSignal> {
        let turn_groups = TurnGroup::for_i(i, map);

        let mut phases = Vec::new();
        let sorted_roads = map
            .get_i(i)
            .get_roads_sorted_by_incoming_angle(map.all_roads());
        for idx in 0..sorted_roads.len() {
            let r = sorted_roads[idx];
            let adj1 = *abstutil::wraparound_get(&sorted_roads, (idx as isize) - 1);
            let adj2 = *abstutil::wraparound_get(&sorted_roads, (idx as isize) + 1);

            let mut phase = Phase::new();
            for group in turn_groups.values() {
                if group.turn_type == TurnType::Crosswalk {
                    if group.id.from == adj1 || group.id.from == adj2 {
                        phase.protected_groups.insert(group.id);
                    }
                } else if group.id.from == r {
                    phase.yield_groups.insert(group.id);
                }
            }
            // Might have a one-way outgoing road. Skip it.
            if !phase.yield_groups.is_empty() {
                phases.push(phase);
            }
        }
        let ts = ControlTrafficSignal {
            id: i,
            phases,
            offset: Duration::ZERO,
            turn_groups,
        };
        ts.validate().ok()
    }

    pub fn convert_to_ped_scramble(&mut self, map: &Map) {
        // Remove Crosswalk groups from existing phases.
        let mut replaced = std::mem::replace(&mut self.phases, Vec::new());
        for phase in replaced.iter_mut() {
            // Crosswalks are only in protected_groups.
            retain_btreeset(&mut phase.protected_groups, |g| {
                self.turn_groups[g].turn_type != TurnType::Crosswalk
            });

            // Blindly try to promote yield groups to protected, now that crosswalks are gone.
            let mut promoted = Vec::new();
            for g in &phase.yield_groups {
                if phase.could_be_protected(*g, &self.turn_groups) {
                    phase.protected_groups.insert(*g);
                    promoted.push(*g);
                }
            }
            for g in promoted {
                phase.yield_groups.remove(&g);
            }
        }
        self.phases = replaced;

        let mut phase = Phase::new();
        for g in self.turn_groups.values() {
            if g.turn_type == TurnType::Crosswalk {
                phase.edit_group(g, TurnPriority::Protected, &self.turn_groups, map);
            }
        }
        self.phases.push(phase);
    }
}

impl Phase {
    pub fn new() -> Phase {
        Phase {
            protected_groups: BTreeSet::new(),
            yield_groups: BTreeSet::new(),
            duration: Duration::seconds(30.0),
        }
    }

    pub fn could_be_protected(
        &self,
        g1: TurnGroupID,
        turn_groups: &BTreeMap<TurnGroupID, TurnGroup>,
    ) -> bool {
        let group1 = &turn_groups[&g1];
        for g2 in &self.protected_groups {
            if g1 == *g2 || group1.conflicts_with(&turn_groups[g2]) {
                return false;
            }
        }
        true
    }

    pub fn get_priority_of_turn(&self, t: TurnID, parent: &ControlTrafficSignal) -> TurnPriority {
        // TODO Cache this?
        let g = parent
            .turn_groups
            .values()
            .find(|g| g.members.contains(&t))
            .map(|g| g.id)
            .unwrap();
        self.get_priority_of_group(g)
    }

    pub fn get_priority_of_group(&self, g: TurnGroupID) -> TurnPriority {
        if self.protected_groups.contains(&g) {
            TurnPriority::Protected
        } else if self.yield_groups.contains(&g) {
            TurnPriority::Yield
        } else {
            TurnPriority::Banned
        }
    }

    pub fn edit_group(
        &mut self,
        g: &TurnGroup,
        pri: TurnPriority,
        turn_groups: &BTreeMap<TurnGroupID, TurnGroup>,
        map: &Map,
    ) {
        let mut ids = vec![g.id];
        if g.turn_type == TurnType::Crosswalk {
            for t in &map.get_t(g.id.crosswalk.unwrap()).other_crosswalk_ids {
                ids.push(
                    *turn_groups
                        .keys()
                        .find(|id| id.crosswalk == Some(*t))
                        .unwrap(),
                );
            }
        }
        for id in ids {
            self.protected_groups.remove(&id);
            self.yield_groups.remove(&id);
            if pri == TurnPriority::Protected {
                self.protected_groups.insert(id);
            } else if pri == TurnPriority::Yield {
                self.yield_groups.insert(id);
            }
        }
    }
}

// Add all possible protected groups to existing phases.
fn expand_all_phases(phases: &mut Vec<Phase>, turn_groups: &BTreeMap<TurnGroupID, TurnGroup>) {
    for phase in phases.iter_mut() {
        for g in turn_groups.keys() {
            if phase.could_be_protected(*g, turn_groups) {
                phase.protected_groups.insert(*g);
            }
        }
    }
}

const PROTECTED: bool = true;
const YIELD: bool = false;

fn make_phases(
    map: &Map,
    i: IntersectionID,
    phase_specs: Vec<Vec<(Vec<RoadID>, TurnType, bool)>>,
) -> Vec<Phase> {
    // TODO Could pass this in instead of recompute...
    let turn_groups = TurnGroup::for_i(i, map);
    let mut phases: Vec<Phase> = Vec::new();

    for specs in phase_specs {
        let mut phase = Phase::new();

        for (roads, turn_type, protected) in specs.into_iter() {
            for group in turn_groups.values() {
                if !roads.contains(&group.id.from) || turn_type != group.turn_type {
                    continue;
                }

                phase.edit_group(
                    group,
                    if protected {
                        TurnPriority::Protected
                    } else {
                        TurnPriority::Yield
                    },
                    &turn_groups,
                    map,
                );
            }
        }

        // Filter out empty phases if they happen.
        if phase.protected_groups.is_empty() && phase.yield_groups.is_empty() {
            continue;
        }

        phases.push(phase);
    }

    phases
}
