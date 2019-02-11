use abstutil::{LogAdapter, Timer};
use log::LevelFilter;
use map_model::{Map, MapEdits};
use structopt::StructOpt;

static LOG_ADAPTER: LogAdapter = LogAdapter;

#[derive(StructOpt)]
#[structopt(name = "precompute")]
struct Flags {
    /// Map
    #[structopt(name = "load")]
    load: String,

    /// Name of map edits. Shouldn't be a full path or have the ".json"
    #[structopt(long = "edits_name")]
    edits_name: String,
}

fn main() {
    log::set_max_level(LevelFilter::Debug);
    log::set_logger(&LOG_ADAPTER).unwrap();

    let flags = Flags::from_args();
    let mut timer = Timer::new(&format!(
        "precompute {} with {}",
        flags.load, flags.edits_name
    ));

    let edits: MapEdits = if flags.edits_name == "no_edits" {
        MapEdits::new(&flags.load)
    } else {
        abstutil::read_json(&format!(
            "../data/edits/{}/{}.json",
            flags.load, flags.edits_name
        ))
        .unwrap()
    };

    let map = Map::new(&flags.load, edits, &mut timer).unwrap();
    timer.start("save map");
    map.save();
    timer.stop("save map");
    timer.done();
}
