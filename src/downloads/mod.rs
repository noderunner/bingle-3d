//! Looks a server offers (Bingle's ADR 0147, *Every client's files*): a game whose `clients`
//! name this one has a look on the server, which the client fetches, checks and keeps, then
//! draws the game with exactly as it would a built-in or private-folder one. So one build of
//! the client plays a private game on any server that offers its look.
//!
//! - **What is fetched**: a game's whole look when the player opens it, unless the client has
//!   one of its own (built in, or in the private folder: that is how a look is made before it
//!   goes into a pack); the appearance file and the picture on its card when the list shows it.
//!   The fetching is the network thread's (`fetch.rs`), kept by hash (`cache.rs`).
//! - **What is taken**: only what keeps to the client's own limits (`check.rs`). A look is data,
//!   never code. Its files are found under `downloads/` by their hashes (`assets.rs`), so it
//!   adds to the client and never replaces a file the client ships or another game's.
//! - **While it is fetched** the game waits to be drawn, and its panel says how far the
//!   fetching has got; the player may play on without it. What happened in the game meanwhile
//!   is read once it is drawn.
//! - **A look the client cannot read** (a later format, a file it refused, a name of something
//!   it lacks) is left out, as a whole, and logged: the game is drawn from the defaults.

pub mod cache;
pub mod check;
pub mod fetch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::prelude::*;

pub use fetch::{News, Scope};

use crate::appearance::{Appearance, Appearances};
use crate::assets;
use crate::browser::CardPictures;
use crate::net::api::{GameDef, GameInfo};
use crate::net::session::Opened;
use crate::net::{self, Connection, Net, Request, ServerStatus, WatchStatus};

pub struct DownloadsPlugin;

impl Plugin for DownloadsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Downloads>().add_systems(
            PreUpdate,
            (
                take_news.run_if(|d: Res<Downloads>| !d.news.is_empty()),
                let_go_of_stale,
                ask_for_pictures.run_if(resource_changed::<ServerStatus>),
            )
                .chain()
                .after(net::poll_network),
        );
    }
}

/// Where each game's look from the server stands in this run, and the game held while its look
/// is fetched.
#[derive(Resource, Debug, Default)]
pub struct Downloads {
    looks: HashMap<String, Look>,
    held: Option<Held>,
    /// What the network thread said, for `take_news`.
    news: Vec<News>,
}

/// A game's look from the server, in this run.
#[derive(Debug, Clone, PartialEq)]
pub enum Look {
    /// Being fetched, so far, of so many bytes to fetch.
    Fetching { scope: Scope, done: u64, total: u64 },
    /// Here: as far as the picture on its card, or the whole look (in `Appearances`).
    Here(Scope),
    /// Left out, and why: the game is drawn from the defaults for the rest of the run.
    LeftOut(String),
    /// The player played on without it; asked for again when the game is opened again.
    Skipped,
}

/// A game opened, waiting for its look to be drawn.
#[derive(Debug)]
struct Held {
    watch: u64,
    opened: Box<Opened>,
    /// Something happened in the game while it waited: read it again once it is drawn.
    missed: bool,
}

impl Downloads {
    /// Whether this game is to wait for its look from the server: the server offers one, the
    /// client has none of its own, and it was not left out before.
    pub fn wants(&self, info: &GameInfo, appearances: &Appearances) -> bool {
        info.clients.iter().any(|c| c == net::api::CLIENT)
            && appearances.for_game(&info.id).is_none()
            && !matches!(self.looks.get(&info.id), Some(Look::LeftOut(_)))
    }

    /// Holds a game opened until its look is here, and asks for it.
    pub fn hold(&mut self, net: &Net, watch: u64, opened: Box<Opened>) {
        let game = opened.info.id.clone();
        info!("fetching the look of {game}");
        self.looks.insert(
            game.clone(),
            Look::Fetching {
                scope: Scope::Whole,
                done: 0,
                total: 0,
            },
        );
        net.send(Request::FetchLook {
            game,
            scope: Scope::Whole,
        });
        self.held = Some(Held {
            watch,
            opened,
            missed: false,
        });
    }

    /// Whether this watch's game is held, waiting for its look.
    pub fn holds(&self, watch: u64) -> bool {
        self.held.as_ref().is_some_and(|h| h.watch == watch)
    }

    /// Something happened in the held game: it is read again once it is drawn.
    pub fn missed(&mut self, watch: u64) {
        if let Some(held) = self.held.as_mut().filter(|h| h.watch == watch) {
            held.missed = true;
        }
    }

    /// How far the look of this watch's game has been fetched, while it waits: bytes fetched,
    /// of so many.
    pub fn waiting(&self, watch: u64) -> Option<(u64, u64)> {
        let held = self.held.as_ref().filter(|h| h.watch == watch)?;
        match self.looks.get(&held.opened.info.id) {
            Some(Look::Fetching { done, total, .. }) => Some((*done, *total)),
            _ => Some((0, 0)),
        }
    }

    /// News from the network thread.
    pub fn push(&mut self, news: News) {
        self.news.push(news);
    }

    /// Plays on without the look: the game is drawn from the defaults now.
    pub fn skip(&mut self, net: &Net, watch: &mut WatchStatus) {
        let Some(held) = self.held.take() else {
            return;
        };
        let game = held.opened.info.id.clone();
        net.send(Request::AbandonLook { game: game.clone() });
        self.looks.insert(game, Look::Skipped);
        draw(net, watch, held);
    }

    #[cfg(test)]
    pub fn look(&self, game: &str) -> Option<&Look> {
        self.looks.get(game)
    }
}

/// Draws a game held for its look, if it is still the one watched.
fn draw(net: &Net, watch: &mut WatchStatus, held: Held) {
    if held.watch != watch.id() {
        return;
    }
    let id = watch.id();
    net::open_table(watch, *held.opened);
    if held.missed {
        net.send(Request::Reload { watch: id });
    }
}

/// Lets go of a game held for its look once it is no longer watched (the player went back),
/// and stops fetching its look.
fn let_go_of_stale(mut downloads: ResMut<Downloads>, net: Res<Net>, watch: Res<WatchStatus>) {
    if downloads
        .held
        .as_ref()
        .is_none_or(|h| h.watch == watch.id())
    {
        return;
    }
    let held = downloads.held.take().unwrap();
    let game = held.opened.info.id.clone();
    net.send(Request::AbandonLook { game: game.clone() });
    downloads.looks.insert(game, Look::Skipped);
}

/// Asks for the picture on the card of each game the list shows whose look the server offers:
/// one game of each family, since a family's members share their files.
fn ask_for_pictures(
    mut downloads: ResMut<Downloads>,
    net: Res<Net>,
    status: Res<ServerStatus>,
    appearances: Res<Appearances>,
) {
    let Connection::Ready(snapshot) = &status.connection else {
        return;
    };
    for def in pictures_wanted(&snapshot.defs, &appearances) {
        if downloads.looks.contains_key(&def.id) {
            continue;
        }
        downloads.looks.insert(
            def.id.clone(),
            Look::Fetching {
                scope: Scope::Picture,
                done: 0,
                total: 0,
            },
        );
        net.send(Request::FetchLook {
            game: def.id.clone(),
            scope: Scope::Picture,
        });
    }
}

/// The first game of each family whose look the server offers, among those the client has no
/// look of its own for.
fn pictures_wanted<'a>(defs: &'a [GameDef], appearances: &Appearances) -> Vec<&'a GameDef> {
    let mut seen = std::collections::HashSet::new();
    defs.iter()
        .filter(|d| d.offers_look() && appearances.for_game(&d.id).is_none())
        .filter(|d| seen.insert(d.family.clone().unwrap_or_else(|| d.id.clone())))
        .collect()
}

// A system: each resource it fills is a parameter.
#[allow(clippy::too_many_arguments)]
fn take_news(
    mut downloads: ResMut<Downloads>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
    mut appearances: ResMut<Appearances>,
    registry: Res<EmbeddedAssetRegistry>,
    server: Res<AssetServer>,
    mut pictures: ResMut<CardPictures>,
) {
    for news in std::mem::take(&mut downloads.news) {
        let (game, scope, result) = match news {
            News::Progress {
                game,
                done: d,
                total: t,
            } => {
                if let Some(Look::Fetching { done, total, .. }) = downloads.looks.get_mut(&game) {
                    (*done, *total) = (d, t);
                }
                continue;
            }
            News::Done {
                game,
                scope,
                result,
            } => (game, scope, result),
        };
        let look = match result {
            Ok(fetched) => {
                for problem in &fetched.refused {
                    warn!("the look of {game}: {problem}");
                }
                let names = register(&registry, fetched.files);
                let (look, picture) = read(&game, scope, &names);
                if let Some(file) = picture {
                    pictures.insert(game.clone(), server.load(assets::path(&file)));
                }
                match (scope, look) {
                    (Scope::Picture, _) => Look::Here(Scope::Picture),
                    (Scope::Whole, Ok(look)) => {
                        info!("the look of {game} is here");
                        appearances.0.push(look);
                        Look::Here(Scope::Whole)
                    }
                    (Scope::Whole, Err(e)) => {
                        warn!("the look of {game} is left out: {e}");
                        Look::LeftOut(e)
                    }
                }
            }
            // A picture the server could not give now is asked for again when the game is
            // opened; a look, the next time it is.
            Err(e) => {
                warn!("the look of {game} could not be fetched: {e}");
                match scope {
                    Scope::Picture => Look::Here(Scope::Picture),
                    Scope::Whole => Look::Skipped,
                }
            }
        };
        // A picture fetched while the whole look was asked for says nothing of the look.
        let fetching_whole = matches!(
            downloads.looks.get(&game),
            Some(Look::Fetching {
                scope: Scope::Whole,
                ..
            })
        );
        if scope == Scope::Whole || !fetching_whole {
            downloads.looks.insert(game.clone(), look);
        }
        if scope == Scope::Whole
            && let Some(held) = downloads.held.take_if(|h| h.opened.info.id == game)
        {
            draw(&net, &mut watch, held);
        }
    }
}

/// Adds the files fetched to the client's, each under `downloads/` by its hash, and gives the
/// name each path of the look's folder is found by.
fn register(registry: &EmbeddedAssetRegistry, files: Vec<fetch::File>) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for file in files {
        let name = format!(
            "{}{}.{}",
            assets::DOWNLOADED,
            file.sha256,
            file.kind.extension()
        );
        let (bytes, new) = assets::add_downloaded(&name, file.bytes);
        if new {
            registry.insert_asset(PathBuf::new(), Path::new(&name), bytes);
        }
        names.insert(file.path, name);
    }
    names
}

/// The look of a game from its files, by their paths (as `register` names them), and the
/// picture on its card, if it has one.
fn read(
    game: &str,
    scope: Scope,
    names: &HashMap<String, String>,
) -> (Result<Appearance, String>, Option<String>) {
    let mut texts: Vec<(String, String)> = names
        .iter()
        .filter(|(path, _)| check::Kind::of(path) == Some(check::Kind::Look))
        .filter_map(|(path, name)| Some((path.clone(), assets::text(name)?.to_string())))
        .collect();
    texts.sort();
    let picture = Appearance::downloaded_picture(game, &texts).and_then(|p| names.get(&p).cloned());
    let look = match scope {
        Scope::Whole => Appearance::downloaded(game, &texts, |p| names.get(p).cloned()),
        Scope::Picture => Err("only its picture was fetched".into()),
    };
    (look, picture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::tests::opened;

    const LOOK: &str = r#"
format = 1
games = ["made-up-1.0.0"]

[pieces.man]
model = "models/man.glb"
material = "jade"

[materials.jade]
albedo = "textures/jade.png"

[card.pictures]
"made-up-1.0.0" = "pictures/card.png"
"#;

    fn file(path: &str, bytes: &[u8]) -> fetch::File {
        fetch::File {
            path: path.into(),
            sha256: cache::sha256(bytes),
            kind: check::Kind::of(path).unwrap(),
            bytes: bytes.to_vec(),
        }
    }

    fn model() -> Vec<u8> {
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "accessors": [{"count": 3, "type": "VEC3", "componentType": 5126,
                "min": [0, 0, 0], "max": [1, 1, 1]}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        });
        check::tests::glb(&json, &[])
    }

    fn app() -> (App, std::sync::mpsc::Receiver<Request>) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let (net, inbox) = Net::for_test();
        app.insert_resource(net)
            .init_resource::<Downloads>()
            .init_resource::<WatchStatus>()
            .init_resource::<CardPictures>()
            .init_resource::<EmbeddedAssetRegistry>()
            .insert_resource(Appearances::default())
            .add_systems(Update, take_news);
        (app, inbox)
    }

    /// Opens a game of the made-up pack in the app's watch, holding it for its look.
    fn hold(app: &mut App) -> u64 {
        let mut opened = opened();
        opened.info.id = "made-up-1.0.0".into();
        opened.info.clients = vec!["bingle-3d".into()];
        let world = app.world_mut();
        let watch = world.resource_mut::<WatchStatus>().start_for_test();
        let appearances = Appearances::default();
        let mut downloads = world.resource_mut::<Downloads>();
        assert!(downloads.wants(&opened.info, &appearances));
        let net = Net::for_test().0;
        downloads.hold(&net, watch, Box::new(opened));
        watch
    }

    fn done(app: &mut App, files: Vec<fetch::File>, refused: Vec<String>) {
        app.world_mut()
            .resource_mut::<Downloads>()
            .push(News::Done {
                game: "made-up-1.0.0".into(),
                scope: Scope::Whole,
                result: Ok(fetch::Fetched { files, refused }),
            });
        app.update();
    }

    #[test]
    fn a_held_game_is_drawn_with_its_look_once_it_is_here() {
        let (mut app, inbox) = app();
        let watch = hold(&mut app);
        app.world_mut().resource_mut::<Downloads>().missed(watch);
        assert!(app.world().resource::<WatchStatus>().table.is_none());
        assert_eq!(
            app.world().resource::<Downloads>().waiting(watch),
            Some((0, 0))
        );
        done(
            &mut app,
            vec![
                file("appearance/made-up.toml", LOOK.as_bytes()),
                file("models/man.glb", &model()),
                file("textures/jade.png", &check::tests::png(8, 8)),
                file("pictures/card.png", &check::tests::png(8, 8)),
            ],
            Vec::new(),
        );
        let world = app.world();
        let table = world
            .resource::<WatchStatus>()
            .table
            .as_ref()
            .expect("drawn");
        assert_eq!(table.info.id, "made-up-1.0.0");
        let look = world
            .resource::<Appearances>()
            .for_game("made-up-1.0.0")
            .unwrap_or_else(|| panic!("{:?}", world.resource::<Downloads>().look("made-up-1.0.0")));
        assert!(look.downloaded);
        let model = &look.pieces["man"].model;
        assert!(
            model.starts_with("downloads/") && model.ends_with(".glb"),
            "{model}"
        );
        assert!(assets::exists(model));
        assert_eq!(assets::model_span(model), Some((0.0, 1.0)));
        assert!(world.resource::<CardPictures>().has("made-up-1.0.0"));
        assert_eq!(
            world.resource::<Downloads>().look("made-up-1.0.0"),
            Some(&Look::Here(Scope::Whole))
        );
        // What happened while it waited is read.
        assert!(
            inbox
                .try_iter()
                .any(|r| matches!(r, Request::Reload { watch: w } if w == watch))
        );
    }

    #[test]
    fn a_look_with_a_file_refused_is_left_out_and_the_game_is_still_drawn() {
        let (mut app, _inbox) = app();
        hold(&mut app);
        done(
            &mut app,
            vec![
                file("appearance/made-up.toml", LOOK.as_bytes()),
                file("pictures/card.png", &check::tests::png(8, 8)),
            ],
            vec!["models/man.glb: over 33554432 bytes".into()],
        );
        let world = app.world();
        assert!(world.resource::<WatchStatus>().table.is_some(), "drawn");
        assert!(
            world
                .resource::<Appearances>()
                .for_game("made-up-1.0.0")
                .is_none()
        );
        assert!(matches!(
            world.resource::<Downloads>().look("made-up-1.0.0"),
            Some(Look::LeftOut(e)) if e.contains("no model downloads/missing/models/man.glb")
        ));
        // Not asked for again in this run.
        let mut opened = opened();
        opened.info.id = "made-up-1.0.0".into();
        opened.info.clients = vec!["bingle-3d".into()];
        assert!(
            !world
                .resource::<Downloads>()
                .wants(&opened.info, &Appearances::default())
        );
    }

    #[test]
    fn a_look_in_a_later_format_is_left_out() {
        let (mut app, _inbox) = app();
        hold(&mut app);
        let later = LOOK.replace("format = 1", "format = 2\nnew_thing = true");
        done(
            &mut app,
            vec![file("appearance/made-up.toml", later.as_bytes())],
            Vec::new(),
        );
        let world = app.world();
        assert!(world.resource::<WatchStatus>().table.is_some(), "drawn");
        assert!(matches!(
            world.resource::<Downloads>().look("made-up-1.0.0"),
            Some(Look::LeftOut(e)) if e.contains("format 2")
        ));
    }

    #[test]
    fn the_player_may_play_on_without_it() {
        let (mut app, _inbox) = app();
        let watch = hold(&mut app);
        let (net, inbox) = Net::for_test();
        let world = app.world_mut();
        world.resource_scope(|world, mut downloads: Mut<Downloads>| {
            downloads.skip(&net, &mut world.resource_mut::<WatchStatus>());
        });
        assert!(world.resource::<WatchStatus>().table.is_some());
        assert!(!world.resource::<Downloads>().holds(watch));
        assert!(
            inbox
                .try_iter()
                .any(|r| matches!(r, Request::AbandonLook { game } if game == "made-up-1.0.0"))
        );
        // Asked for again the next time the game is opened.
        let mut info = opened().info;
        info.id = "made-up-1.0.0".into();
        info.clients = vec!["bingle-3d".into()];
        assert!(
            world
                .resource::<Downloads>()
                .wants(&info, &Appearances::default())
        );
    }

    /// The look `scripts/pack-server.sh` gives Bingle's Draughts fixture pack: every file is
    /// taken, and the look reads whole.
    #[test]
    fn the_test_packs_look_is_taken_whole() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pack/clients/bingle-3d");
        let mut files = Vec::new();
        for folder in ["appearance", "pictures", "textures"] {
            for entry in std::fs::read_dir(dir.join(folder)).unwrap() {
                let path = format!("{folder}/{}", entry.unwrap().file_name().to_string_lossy());
                let kind = check::path(&path).unwrap().unwrap();
                let bytes = std::fs::read(dir.join(&path)).unwrap();
                assert_eq!(check::contents(kind, &bytes), Ok(()), "{path}");
                files.push(file(&path, &bytes));
            }
        }
        let names = register(&EmbeddedAssetRegistry::default(), files);
        for game in ["pack-draughts-0.1.0", "pack-draughts-0.1.0-6"] {
            let (look, picture) = read(game, Scope::Whole, &names);
            let look = look.unwrap();
            assert_eq!(look.games, [game]);
            assert!(
                look.materials["jade"]
                    .albedo
                    .as_ref()
                    .unwrap()
                    .starts_with("downloads/")
            );
            assert!(
                picture.is_some(),
                "{game}: the only picture is every variant's"
            );
        }
    }

    #[test]
    fn a_picture_is_asked_for_once_for_each_family_the_client_has_no_look_for() {
        let def = |id: &str, family: Option<&str>, clients: &[&str]| GameDef {
            id: id.into(),
            name: id.into(),
            seats_required: 2,
            family: family.map(Into::into),
            description: String::new(),
            prototype: false,
            version: "1.0.0".into(),
            tutorials: 0,
            private: true,
            clients: clients.iter().map(|c| c.to_string()).collect(),
        };
        let defs = [
            def("hex-11", None, &["bingle-3d"]),
            def("p-1.0.0", Some("p"), &["bingle-3d"]),
            def("p-1.0.0-big", Some("p"), &["bingle-3d"]),
            def("q", None, &["made-up"]),
            def("r", None, &["bingle-3d"]),
        ];
        let wanted: Vec<&str> = pictures_wanted(&defs, &Appearances::built_in())
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(
            wanted,
            ["p-1.0.0", "r"],
            "hex has a look of the client's own"
        );
    }
}
