use crate::{Scenario, Sim, SimOptions};
use abstutil::CmdArgs;
use geom::Duration;
use map_model::{Map, MapEdits};
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;

#[derive(Clone)]
pub struct SimFlags {
    pub load: String,
    pub use_map_fixes: bool,
    pub rng_seed: Option<u8>,
    pub opts: SimOptions,
}

impl SimFlags {
    pub fn from_args(args: &mut CmdArgs) -> SimFlags {
        SimFlags {
            load: args
                .optional_free()
                .unwrap_or_else(|| "../data/system/maps/montlake.bin".to_string()),
            use_map_fixes: !args.enabled("--nofixes"),
            rng_seed: args.optional_parse("--rng_seed", |s| s.parse()),
            opts: SimOptions {
                run_name: args
                    .optional("--run_name")
                    .unwrap_or_else(|| "unnamed".to_string()),
                savestate_every: args.optional_parse("--savestate_every", Duration::parse),
                use_freeform_policy_everywhere: args.enabled("--freeform_policy"),
                disable_block_the_box: args.enabled("--disable_block_the_box"),
                recalc_lanechanging: !args.enabled("--dont_recalc_lc"),
            },
        }
    }

    // TODO rename seattle_test
    pub fn for_test(run_name: &str) -> SimFlags {
        SimFlags::synthetic_test("montlake", run_name)
    }

    pub fn synthetic_test(map: &str, run_name: &str) -> SimFlags {
        SimFlags {
            load: abstutil::path_map(map),
            use_map_fixes: true,
            rng_seed: Some(42),
            opts: SimOptions::new(run_name),
        }
    }

    pub fn make_rng(&self) -> XorShiftRng {
        if let Some(seed) = self.rng_seed {
            XorShiftRng::from_seed([seed; 16])
        } else {
            XorShiftRng::from_entropy()
        }
    }

    // Convenience method to setup everything.
    pub fn load(&self, timer: &mut abstutil::Timer) -> (Map, Sim, XorShiftRng) {
        let mut rng = self.make_rng();

        let mut opts = self.opts.clone();

        if self.load.starts_with("../data/player/save/") {
            timer.note(format!("Resuming from {}", self.load));

            let sim: Sim = abstutil::read_binary(self.load.clone(), timer);

            let mut map = Map::new(abstutil::path_map(&sim.map_name), false, timer);
            map.apply_edits(
                MapEdits::load(map.get_name(), &sim.edits_name, timer),
                timer,
            );
            map.mark_edits_fresh();
            map.recalculate_pathfinding_after_edits(timer);

            (map, sim, rng)
        } else if self.load.starts_with("../data/system/scenarios/") {
            timer.note(format!(
                "Seeding the simulation from scenario {}",
                self.load
            ));

            let scenario: Scenario = abstutil::read_binary(self.load.clone(), timer);

            let map = Map::new(abstutil::path_map(&scenario.map_name), false, timer);

            if opts.run_name == "unnamed" {
                opts.run_name = scenario.scenario_name.clone();
            }
            let mut sim = Sim::new(&map, opts, timer);
            scenario.instantiate(&mut sim, &map, &mut rng, timer);

            (map, sim, rng)
        } else if self.load.starts_with(&abstutil::path_all_raw_maps())
            || self.load.starts_with(&abstutil::path_all_synthetic_maps())
        {
            timer.note(format!("Loading map {}", self.load));

            let map = Map::new(self.load.clone(), self.use_map_fixes, timer);

            timer.start("create sim");
            let sim = Sim::new(&map, opts, timer);
            timer.stop("create sim");

            (map, sim, rng)
        } else if self.load.starts_with(&abstutil::path_all_maps()) {
            timer.note(format!("Loading map {}", self.load));

            let map = Map::new(self.load.clone(), false, timer);

            timer.start("create sim");
            let sim = Sim::new(&map, opts, timer);
            timer.stop("create sim");

            (map, sim, rng)
        } else {
            panic!("Don't know how to load {}", self.load);
        }
    }
}
