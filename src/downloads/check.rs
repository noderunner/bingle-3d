//! What this client takes of a look a server offers: everything from a server is hostile until
//! it is checked (Bingle's ADR 0147), so these are the client's own limits, whatever the
//! server's were. A file is held to them twice: before it is fetched, by what the index says of
//! it, and once its bytes are here, by what they are.
//!
//! A look is data and never code: the only kinds taken are appearance files, binary glTF models,
//! PNG, JPEG and KTX2 pictures and Ogg Vorbis sounds. No shader, no script, nothing that runs.
//! Every other file of the folder is left where it is.

use crate::assets;
use crate::net::api::ClientFile;

/// The most files of one game's look the client takes.
pub const MAX_FILES: usize = 1024;
/// The most bytes of one game's look, all its files together.
pub const MAX_BYTES: u64 = 256 << 20;
/// The longest path in a look's folder, in bytes.
pub const MAX_PATH: usize = 200;
/// The widest and tallest picture, in pixels: a texture, or one inside a model.
pub const MAX_SIDE: u32 = 4096;
/// The most vertices of one model, all its meshes together.
pub const MAX_VERTICES: u64 = 500_000;
/// The most elements of any one accessor of a model (indices, as well as vertices).
pub const MAX_ACCESSOR: u64 = 4_000_000;
/// The longest sound, in seconds.
pub const MAX_SECONDS: f64 = 60.0;

/// A kind of file the client takes from a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An appearance file, `appearance/*.toml`.
    Look,
    /// A binary glTF model.
    Model,
    Png,
    Jpeg,
    Ktx2,
    /// Ogg Vorbis.
    Sound,
}

impl Kind {
    /// The kind of a file by its path: an appearance file only in `appearance/`, the rest by
    /// their extension. None for a file the client does not read (notes, JSON).
    pub fn of(path: &str) -> Option<Kind> {
        let (_, ext) = path.rsplit_once('.')?;
        Some(match ext.to_ascii_lowercase().as_str() {
            "toml" if path.starts_with("appearance/") && !path[11..].contains('/') => Kind::Look,
            "glb" => Kind::Model,
            "png" => Kind::Png,
            "jpg" | "jpeg" => Kind::Jpeg,
            "ktx2" => Kind::Ktx2,
            "ogg" => Kind::Sound,
            _ => return None,
        })
    }

    /// The most bytes a file of this kind may have.
    pub fn limit(self) -> u64 {
        match self {
            Kind::Look => 1 << 20,
            Kind::Sound => 8 << 20,
            Kind::Model | Kind::Png | Kind::Jpeg | Kind::Ktx2 => 32 << 20,
        }
    }

    /// The extension the client names a file of this kind by, which is what tells Bevy's asset
    /// server how to read it.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Look => "toml",
            Kind::Model => "glb",
            Kind::Png => "png",
            Kind::Jpeg => "jpg",
            Kind::Ktx2 => "ktx2",
            Kind::Sound => "ogg",
        }
    }
}

/// Whether a path in an index may be fetched, and as what: None for a file the client does not
/// read, which is left on the server. A path must be one any system can name a file by, and
/// a file a look names (all but the appearance files, which nothing names) none of the
/// client's own or of its private folder's: a downloaded look adds to the client, and never
/// replaces a file it ships or another game's.
pub fn path(path: &str) -> Result<Option<Kind>, String> {
    if path.len() > MAX_PATH {
        return Err(format!("a path of over {MAX_PATH} bytes"));
    }
    let named = |s: &str| {
        s.bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    if !path.split('/').all(named) {
        return Err("not a path in a look's folder".into());
    }
    let kind = Kind::of(path);
    let ours = assets::FILES.iter().any(|(f, _)| *f == path) || assets::is_private(path);
    if ours && kind != Some(Kind::Look) {
        return Err("the client has its own, so this one is left out".into());
    }
    Ok(kind)
}

/// The files of an index to take, and what was wrong with the others (each with its path): a
/// path or a hash that cannot be, a file larger than its kind may be, and the files past the
/// count or the size a look may have.
pub fn index(files: &[ClientFile]) -> (Vec<(ClientFile, Kind)>, Vec<String>) {
    let (mut taken, mut problems) = (Vec::new(), Vec::new());
    let mut bytes = 0;
    for file in files {
        let kind = match path(&file.path) {
            Ok(Some(kind)) => kind,
            Ok(None) => continue,
            Err(e) => {
                problems.push(format!("{}: {e}", file.path));
                continue;
            }
        };
        let hex = file.sha256.len() == 64
            && file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let problem = if !hex {
            Some("its hash is not a SHA-256".to_string())
        } else if file.size > kind.limit() {
            Some(format!("over {} bytes", kind.limit()))
        } else if taken.len() == MAX_FILES {
            Some(format!("past the {MAX_FILES} files a look may have"))
        } else if bytes + file.size > MAX_BYTES {
            Some(format!("past the {MAX_BYTES} bytes a look may have"))
        } else {
            None
        };
        match problem {
            Some(p) => problems.push(format!("{}: {p}", file.path)),
            None => {
                bytes += file.size;
                taken.push((file.clone(), kind));
            }
        }
    }
    (taken, problems)
}

/// Whether these bytes are a file of this kind that keeps to the client's limits on what its
/// decoder is given: a picture's size, a model's vertices and that it names no file outside
/// itself, a sound's length.
pub fn contents(kind: Kind, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > kind.limit() {
        return Err(format!("over {} bytes", kind.limit()));
    }
    match kind {
        Kind::Look => match std::str::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => Ok(()),
            _ => Err("not text".into()),
        },
        Kind::Model => model(bytes),
        Kind::Png => side(png_size(bytes).ok_or("not a PNG picture")?),
        Kind::Jpeg => side(jpeg_size(bytes).ok_or("not a JPEG picture")?),
        Kind::Ktx2 => ktx2(bytes),
        Kind::Sound => sound(bytes),
    }
}

fn side((width, height): (u32, u32)) -> Result<(), String> {
    if width == 0 || height == 0 {
        Err("a picture of no size".into())
    } else if width > MAX_SIDE || height > MAX_SIDE {
        Err(format!(
            "a picture of {width}×{height}, over {MAX_SIDE} pixels a side"
        ))
    } else {
        Ok(())
    }
}

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u16_be(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// A PNG's width and height, from its header.
pub fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") || b.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((u32_be(b, 16)?, u32_be(b, 20)?))
}

/// A JPEG's width and height, from its first frame's header.
pub fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = 2;
    loop {
        if *b.get(at)? != 0xff {
            return None;
        }
        // Any number of fill bytes may stand before a marker.
        while *b.get(at + 1)? == 0xff {
            at += 1;
        }
        let marker = *b.get(at + 1)?;
        match marker {
            0x01 | 0xd0..=0xd7 => at += 2,
            0xd8..=0xda => return None,
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let height = u16_be(b, at + 5)?;
                let width = u16_be(b, at + 7)?;
                return Some((width.into(), height.into()));
            }
            _ => at += 2 + usize::from(u16_be(b, at + 2)?),
        }
    }
}

fn ktx2(b: &[u8]) -> Result<(), String> {
    const SIGNATURE: &[u8] = b"\xabKTX 20\xbb\r\n\x1a\n";
    let read = || -> Option<[u32; 6]> {
        if !b.starts_with(SIGNATURE) {
            return None;
        }
        let mut out = [0; 6];
        for (i, v) in out.iter_mut().enumerate() {
            *v = u32_le(b, 20 + 4 * i)?;
        }
        Some(out)
    };
    let [width, height, depth, layers, faces, levels] = read().ok_or("not a KTX2 picture")?;
    side((width, height))?;
    if depth > 1 || layers > 1 || !matches!(faces, 1 | 6) || levels > 13 {
        return Err("a KTX2 picture of a shape the client does not draw".into());
    }
    Ok(())
}

/// An Ogg Vorbis sound: its first packet says its rate, and its last page how many samples
/// came before its end.
fn sound(b: &[u8]) -> Result<(), String> {
    let not = || "not an Ogg Vorbis sound".to_string();
    if !b.starts_with(b"OggS") {
        return Err(not());
    }
    let segments = usize::from(*b.get(26).ok_or_else(not)?);
    let packet = b.get(27 + segments..).ok_or_else(not)?;
    if packet.get(..7) != Some(b"\x01vorbis") {
        return Err(not());
    }
    let channels = *packet.get(11).ok_or_else(not)?;
    let rate = u32_le(packet, 12).ok_or_else(not)?;
    if channels == 0 || channels > 8 || rate == 0 || rate > 192_000 {
        return Err(not());
    }
    // A page is at most 65,307 bytes, so the last one starts in the last 64 KiB and a little.
    let tail = b.len().saturating_sub(65_536 + 27);
    let last = b[tail..]
        .windows(4)
        .rposition(|w| w == b"OggS")
        .map(|i| tail + i)
        .ok_or_else(not)?;
    let granule = i64::from_le_bytes(
        b.get(last + 6..last + 14)
            .ok_or_else(not)?
            .try_into()
            .map_err(|_| not())?,
    );
    let seconds = granule as f64 / f64::from(rate);
    if !(0.0..=MAX_SECONDS).contains(&seconds) {
        return Err(format!("a sound of {seconds:.0} s, over {MAX_SECONDS} s"));
    }
    Ok(())
}

/// A binary glTF 2.0 model of its own length, whose buffers and pictures are all inside it
/// (none names a file or a URL, which the asset server would follow), whose pictures are PNG or
/// JPEG within the limits, and whose meshes are not too large to draw.
fn model(b: &[u8]) -> Result<(), String> {
    let not = || "not a binary glTF 2.0 model".to_string();
    if !b.starts_with(b"glTF") || u32_le(b, 4) != Some(2) || u32_le(b, 8) != Some(b.len() as u32) {
        return Err(not());
    }
    let json_len = u32_le(b, 12).ok_or_else(not)? as usize;
    if b.get(16..20) != Some(b"JSON") {
        return Err(not());
    }
    let json = b.get(20..20 + json_len).ok_or_else(not)?;
    let rest = &b[20 + json_len..];
    let bin: &[u8] = if rest.is_empty() {
        &[]
    } else {
        let len = u32_le(rest, 0).ok_or_else(not)? as usize;
        if rest.get(4..8) != Some(b"BIN\0") {
            return Err(not());
        }
        rest.get(8..8 + len).ok_or_else(not)?
    };
    let doc: serde_json::Value = serde_json::from_slice(json).map_err(|_| not())?;
    let list = |key: &str| doc[key].as_array().map(Vec::as_slice).unwrap_or_default();
    let buffers = list("buffers");
    if buffers.len() > 1 || buffers.iter().any(|b| b.get("uri").is_some()) {
        return Err("a model that names a file outside itself".into());
    }
    let views = list("bufferViews");
    for image in list("images") {
        if image.get("uri").is_some() {
            return Err("a model that names a file outside itself".into());
        }
        let view = image["bufferView"]
            .as_u64()
            .and_then(|v| views.get(v as usize))
            .ok_or_else(not)?;
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = view["byteLength"].as_u64().ok_or_else(not)? as usize;
        let bytes = bin.get(start..start + len).ok_or_else(not)?;
        let size = match image["mimeType"].as_str() {
            Some("image/png") => png_size(bytes),
            Some("image/jpeg") => jpeg_size(bytes),
            _ => return Err("a model with a picture that is not PNG or JPEG".into()),
        };
        side(size.ok_or_else(not)?)?;
    }
    let accessors = list("accessors");
    let count = |a: &serde_json::Value| a["count"].as_u64().unwrap_or(0);
    if accessors.iter().any(|a| count(a) > MAX_ACCESSOR) {
        return Err(format!(
            "a model with over {MAX_ACCESSOR} elements in one list"
        ));
    }
    let vertices: u64 = list("meshes")
        .iter()
        .flat_map(|m| {
            m["primitives"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
        })
        .filter_map(|p| p["attributes"]["POSITION"].as_u64())
        .filter_map(|a| accessors.get(a as usize))
        .map(count)
        .sum();
    if vertices > MAX_VERTICES {
        return Err(format!(
            "a model of {vertices} vertices, over {MAX_VERTICES}"
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG's first bytes, of this size: as far as its header.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(width.to_be_bytes());
        b.extend(height.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    /// A binary glTF model of this JSON, with this binary chunk.
    pub(crate) fn glb(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
        let mut json = serde_json::to_vec(json).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bin = bin.to_vec();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let mut b = b"glTF\x02\0\0\0\0\0\0\0".to_vec();
        b.extend((json.len() as u32).to_le_bytes());
        b.extend(b"JSON");
        b.extend(json);
        if !bin.is_empty() {
            b.extend((bin.len() as u32).to_le_bytes());
            b.extend(b"BIN\0");
            b.extend(bin);
        }
        let len = b.len() as u32;
        b[8..12].copy_from_slice(&len.to_le_bytes());
        b
    }

    /// An Ogg Vorbis sound's first page and a last one, which says it is this long.
    pub(crate) fn ogg(seconds: u32) -> Vec<u8> {
        let rate: u32 = 44_100;
        let page = |granule: i64, packet: &[u8]| {
            let mut b = b"OggS\0\x02".to_vec();
            b.extend(granule.to_le_bytes());
            b.extend([0; 12]);
            b.push(1);
            b.push(packet.len() as u8);
            b.extend(packet);
            b
        };
        let mut head = b"\x01vorbis\0\0\0\0\x02".to_vec();
        head.extend(rate.to_le_bytes());
        head.extend([0; 14]);
        let mut b = page(0, &head);
        b.extend(page(i64::from(seconds * rate), b"\x05more"));
        b
    }

    fn file(path: &str, size: u64) -> ClientFile {
        ClientFile {
            path: path.into(),
            size,
            sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn only_the_kinds_the_client_reads_are_fetched_and_only_from_paths_of_its_own() {
        assert_eq!(path("appearance/zertz.toml"), Ok(Some(Kind::Look)));
        assert_eq!(path("models/ring_2.glb"), Ok(Some(Kind::Model)));
        assert_eq!(path("textures/a.JPEG"), Ok(Some(Kind::Jpeg)));
        assert_eq!(path("sounds/click.ogg"), Ok(Some(Kind::Sound)));
        assert_eq!(path("notes.md"), Ok(None), "left on the server");
        assert_eq!(path("look.toml"), Ok(None), "a look is in appearance/");
        assert_eq!(path("appearance/old/a.toml"), Ok(None));
        for bad in [
            "../appearance/hex.toml",
            "/etc/passwd",
            "models//a.glb",
            "models/.a.glb",
            "models\\a.glb",
            "models/a b.glb",
            "",
        ] {
            assert!(path(bad).is_err(), "{bad}");
        }
        assert!(path(&format!("models/{}.glb", "a".repeat(200))).is_err());
        let ours = path("models/ring.glb").unwrap_err();
        assert!(ours.contains("its own"), "{ours}");
        assert_eq!(
            path("appearance/hex.toml"),
            Ok(Some(Kind::Look)),
            "nothing names an appearance file, so a pack's may share our name"
        );
    }

    #[test]
    fn an_index_is_held_to_the_clients_limits_whatever_the_servers_were() {
        let mut bad_hash = file("models/b.glb", 10);
        bad_hash.sha256 = "AB".repeat(32);
        let files = [
            file("appearance/a.toml", 100),
            file("appearance/big.toml", 2 << 20),
            file("models/ring.glb", 10),
            bad_hash,
            file("README.md", 10),
            file("models/a.glb", 10),
        ];
        let (taken, problems) = index(&files);
        let paths: Vec<&str> = taken.iter().map(|(f, _)| f.path.as_str()).collect();
        assert_eq!(paths, ["appearance/a.toml", "models/a.glb"]);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("appearance/big.toml: over"));
        // Too many, and too much.
        let many: Vec<ClientFile> = (0..MAX_FILES + 2)
            .map(|i| file(&format!("models/m{i}.glb"), 1))
            .collect();
        let (taken, problems) = index(&many);
        assert_eq!((taken.len(), problems.len()), (MAX_FILES, 2));
        let heavy: Vec<ClientFile> = (0..10)
            .map(|i| file(&format!("models/m{i}.glb"), 30 << 20))
            .collect();
        let (taken, problems) = index(&heavy);
        assert_eq!((taken.len(), problems.len()), (8, 2));
    }

    #[test]
    fn a_picture_is_held_to_its_size() {
        assert_eq!(contents(Kind::Png, &png(1024, 512)), Ok(()));
        assert!(
            contents(Kind::Png, &png(8192, 16))
                .unwrap_err()
                .contains("8192×16")
        );
        assert!(contents(Kind::Png, &png(0, 16)).is_err());
        assert!(contents(Kind::Png, b"GIF89a").is_err());
        assert!(
            contents(Kind::Jpeg, &png(16, 16)).is_err(),
            "not what it says"
        );
        // A JPEG: start, an APP0 segment, then its frame.
        let jpeg = |w: u16, h: u16| {
            let mut b = vec![
                0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xff, 0xc0, 0, 11, 8,
            ];
            b.extend(h.to_be_bytes());
            b.extend(w.to_be_bytes());
            b.extend([3, 0, 0, 0]);
            b
        };
        assert_eq!(jpeg_size(&jpeg(640, 480)), Some((640, 480)));
        assert_eq!(contents(Kind::Jpeg, &jpeg(640, 480)), Ok(()));
        assert!(contents(Kind::Jpeg, &jpeg(5000, 10)).is_err());
        // KTX2: a cubemap is six faces, and nothing has depth.
        let ktx2 = |w: u32, depth: u32, faces: u32| {
            let mut b = b"\xabKTX 20\xbb\r\n\x1a\n".to_vec();
            b.extend([0; 8]);
            for v in [w, w, depth, 0, faces, 10] {
                b.extend(v.to_le_bytes());
            }
            b
        };
        assert_eq!(contents(Kind::Ktx2, &ktx2(1024, 0, 6)), Ok(()));
        assert!(contents(Kind::Ktx2, &ktx2(1024, 4, 1)).is_err());
        assert!(contents(Kind::Ktx2, &ktx2(1024, 0, 3)).is_err());
        assert!(contents(Kind::Ktx2, &ktx2(8192, 0, 1)).is_err());
    }

    #[test]
    fn a_sound_is_held_to_its_length() {
        assert_eq!(contents(Kind::Sound, &ogg(2)), Ok(()));
        assert!(
            contents(Kind::Sound, &ogg(600))
                .unwrap_err()
                .contains("600 s")
        );
        assert!(contents(Kind::Sound, b"OggS\0\x02").is_err());
        let mut opus = ogg(2);
        opus[29..35].copy_from_slice(b"OpusHe");
        assert!(contents(Kind::Sound, &opus).is_err(), "not Vorbis");
    }

    #[test]
    fn a_model_holds_all_it_needs_and_names_nothing_outside_itself() {
        use serde_json::json;
        let picture = png(64, 64);
        let mesh = |vertices: u64| {
            json!({
                "asset": {"version": "2.0"},
                "buffers": [{"byteLength": 64}],
                "bufferViews": [{"buffer": 0, "byteLength": picture.len()}],
                "images": [{"bufferView": 0, "mimeType": "image/png"}],
                "accessors": [{"count": vertices, "type": "VEC3", "componentType": 5126}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            })
        };
        assert_eq!(contents(Kind::Model, &glb(&mesh(100), &picture)), Ok(()));
        assert!(
            contents(Kind::Model, &glb(&mesh(MAX_VERTICES + 1), &picture))
                .unwrap_err()
                .contains("vertices")
        );
        let mut outside = mesh(100);
        outside["buffers"][0]["uri"] = json!("../../appearance/hex.toml");
        assert!(contents(Kind::Model, &glb(&outside, &picture)).is_err());
        let mut outside = mesh(100);
        outside["images"][0] = json!({"uri": "https://example.com/a.png"});
        assert!(contents(Kind::Model, &glb(&outside, &picture)).is_err());
        assert!(
            contents(Kind::Model, &glb(&mesh(100), &png(5000, 5000))).is_err(),
            "its picture is too large"
        );
        let mut short = glb(&mesh(100), &picture);
        short.pop();
        assert!(contents(Kind::Model, &short).is_err(), "not its own length");
        assert!(contents(Kind::Model, b"glTF").is_err());
        // The client's own models pass.
        for (f, bytes) in assets::FILES.iter().filter(|(f, _)| f.ends_with(".glb")) {
            assert_eq!(contents(Kind::Model, bytes), Ok(()), "{f}");
        }
    }

    #[test]
    fn the_clients_own_files_keep_to_its_limits() {
        for (f, bytes) in assets::FILES {
            if let Some(kind) = Kind::of(f).filter(|k| *k != Kind::Look) {
                assert_eq!(contents(kind, bytes), Ok(()), "{f}");
            }
        }
    }
}
