# Developer guide

## Getting started

You will first need:

- Standard dependencies: `bash`, `curl`, `unzip`, `gunzip`
- Rust, at least 1.38. https://www.rust-lang.org/tools/install

One-time setup:

1.  Download the repository:
    `git clone https://github.com/dabreegster/abstreet.git`

2.  Build all input data. This is very slow, so you should seed from a pre-built
    copy: `./data/grab_seed_data.sh`. This will download about 1GB and expand to
    about 5GB.

3.  Run the game: `cd game; cargo run --release`

## Development tips

- Compile faster by just doing `cargo run`. The executable will have debug stack
  traces and run more slowly. You can do `cargo run --release` to build in
  optimized release mode; compilation will be slower, but the executable much
  faster.
- Some in-game features are turned off by default or don't have a normal menu to access them. The list:
  - To toggle developer mode: press **Control+S** in game, or `cargo run -- --dev`
  - To warp to an object by numeric ID: press **j**
  - To enter debug mode with all sorts of goodies: press **Control+D**
- All code is automatically formatted using
  https://github.com/rust-lang/rustfmt; please run `cargo fmt` before sending a
  PR.
- More random notes [here](/docs/misc_dev_tricks.md)

## Building map data

You can skip this section if you're just touching code in `game`, `ezgui`, and
`sim`.

You'll need some extra dependencies:

- `osmconvert`: See https://wiki.openstreetmap.org/wiki/Osmconvert#Download
- `cs2cs` from proj4: See https://proj.org

The seed data from `data/grab_seed_data.sh` can be built from scratch by doing
`./import.sh && ./precompute.sh --release`. This takes a while.

Some tips:

- If you're modifying the initial OSM data -> RawMap conversion in
  `convert_osm`, then you do need to rerun `./import.sh` and `precompute.sh` to
  regenerate the map.
- If you're modifying `map_model` but not the OSM -> RawMap conversion, then you
  can just do `precompute.sh`.
- Both of those scripts can just regenerate a single map, which is much faster:
  `./import.sh caphill; ./precompute.sh caphill`

## Understanding stuff

The docs listed at
https://github.com/dabreegster/abstreet#documentation-for-developers explain
things like map importing and how the traffic simulation works.

### Code organization

If you're going to dig into the code, it helps to know what all the crates are.
The most interesting crates are `map_model`, `sim`, and `game`.

Constructing the map:

- `convert_osm`: extract useful data from OpenStreetMap and other data sources,
  emit intermediate map format
- `gtfs`: simple library to just extract coordinates of bus stops
- `kml`: extract shapes from KML shapefiles
- `map_model`: the final representation of the map, also conversion from the
  intermediate map format into the final format
- `precompute`: small tool to run the second stage of map conversion and write
  final output
- `popdat`: importing daily trips from PSRC's Soundcast model, specific to
  Seattle
- `map_editor`: GUI for modifying geometry of maps and creating maps from
  scratch

Traffic simulation:

- `sim`: all of the agent-based simulation logic
- `headless`: tool to run a simulation without any visualization

Graphics:

- `game`: the GUI and main gameplay
- `ezgui`: a GUI and 2D OpenGL rendering library, using glium + winit + glutin

Common utilities:

- `abstutil`: a grab-bag of IO helpers, timing and logging utilities, etc
- `geom`: types for GPS and map-space points, lines, angles, polylines,
  polygons, circles, durations, speeds
- `tests`: a custom test runner and some (ehem broken) tests using it

## Example guide for implementing a new feature

A/B Street's transit modeling only includes buses as of September 2019. If you
wanted to start modeling light rail, you'd have to touch many layers of the
code. This is a nice, hefty starter project to understand how everything works.
For now, this is just an initial list of considerations -- I haven't designed or
implemented this yet.

Poking around the .osm extracts in `data/input/osm/`, you'll see a promising
relation with `route = light_rail`. The relation points to individual points
(nodes) as stops, and segments of the track (ways). These need to be represented
in the initial version of the map, `RawMap`, and the final version, `Map`.
Stations probably coincide with existing buildings, and tracks could probably be
modeled as a special type of road. To remember the order of stations and group
everything, there's already a notion of bus route from the `gtfs` crate that
probably works. The `convert_osm` crate is the place to extract this new data
from OSM. It might be worth thinking about how the light rail line gets clipped,
since most maps won't include all of the stations -- should those maps just
terminate trains at the stations, or should trains go to and from the map
border?

Then there are some rendering questions. How should special buildings that act
as light rail stations be displayed? What about the track between stations, and
how to draw trains moving on the track? The track is sometimes underground,
sometimes at-grade with the road (like near Colombia City -- there it even has
to somehow be a part of the existing intersections!), and sometimes over the
road. How to draw it without being really visually noisy with existing stuff on
the ground? Should trains between stations even be drawn at all, or should
hovering over stations show some kind of ETA?

For modeling the movement of the trains along the track, I'd actually recommend
using the existing driving model. Tracks can be a new `LaneType` (that gets
rendered as nice train tracks, probably), and trains can be a new `VehicleType`.
This way, trains queueing happens for free. There's even existing logic to make
buses wait at bus stops and load passengers; maybe that should be extended to
load passengers from a building? How should passengers walking to the platform
be modeled and rendered -- it takes a few minutes sometimes!

Finally, you'll need to figure out how to make some trips incorporate light
rail. Pedestrian trips have the option to use transit or not -- if light rail is
modeled properly, it hopefully fits into the existing transit pathfinding and
everything, so it'll just naturally happen.
