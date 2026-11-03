//! The module the manifest names: the committed tabs/github.wasm is a
//! WebAssembly module of the ABI Hangar reads, built for
//! wasm32-unknown-unknown (it imports `hangar.log` and `hangar.now` and
//! nothing else, no WASI), and both tabs name it. Whether it is the module
//! this source builds is CI's to say (scripts/build-native.sh, then git diff).

use serde_json::Value;

const MODULE: &[u8] = include_bytes!("../../tabs/github.wasm");
const MANIFEST: &str = include_str!("../../hangar-kit.json");

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> u8 {
        let byte = self.bytes[self.at];
        self.at += 1;
        byte
    }

    /// An unsigned LEB128.
    fn leb(&mut self) -> usize {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let byte = self.byte();
            value |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    }

    fn name(&mut self) -> String {
        let len = self.leb();
        let name = String::from_utf8(self.bytes[self.at..self.at + len].to_vec()).unwrap();
        self.at += len;
        name
    }

    fn limits(&mut self) {
        let flags = self.leb();
        self.leb();
        if flags & 1 == 1 {
            self.leb();
        }
    }
}

/// An import: module, field, kind of thing. An export: name, kind.
type Import = (String, String, u8);
type Export = (String, u8);

/// What the module imports and exports, from its sections.
fn interface() -> (Vec<Import>, Vec<Export>) {
    assert_eq!(&MODULE[..4], b"\0asm", "a WebAssembly module");
    assert_eq!(&MODULE[4..8], [1, 0, 0, 0], "version 1");
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut at = 8;
    while at < MODULE.len() {
        let mut head = Reader {
            bytes: MODULE,
            at: at + 1,
        };
        let id = MODULE[at];
        let size = head.leb();
        let start = head.at;
        let mut section = Reader {
            bytes: MODULE,
            at: start,
        };
        match id {
            2 => {
                for _ in 0..section.leb() {
                    let (module, field, kind) = (section.name(), section.name(), section.byte());
                    match kind {
                        0 => {
                            section.leb();
                        }
                        1 => {
                            section.byte();
                            section.limits();
                        }
                        2 => section.limits(),
                        3 => {
                            section.byte();
                            section.byte();
                        }
                        other => panic!("an import of kind {other}"),
                    }
                    imports.push((module, field, kind));
                }
            }
            7 => {
                for _ in 0..section.leb() {
                    let (name, kind) = (section.name(), section.byte());
                    section.leb();
                    exports.push((name, kind));
                }
            }
            _ => {}
        }
        at = start + size;
    }
    (imports, exports)
}

#[test]
fn the_module_imports_hangar_log_and_now_and_nothing_else() {
    let (imports, _) = interface();
    let mut names: Vec<String> = imports
        .iter()
        .map(|(module, field, kind)| format!("{module}.{field}:{kind}"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["hangar.log:0", "hangar.now:0"],
        "no WASI, no wasm-bindgen"
    );
}

#[test]
fn the_module_exports_what_the_abi_asks_for() {
    let (_, exports) = interface();
    let has = |name: &str, kind: u8| {
        exports
            .iter()
            .any(|(export, k)| export == name && *k == kind)
    };
    for function in [
        "hangar_abi_version",
        "hangar_alloc",
        "hangar_init",
        "hangar_update",
        "hangar_view",
    ] {
        assert!(has(function, 0), "{function}");
    }
    assert!(has("memory", 2));
}

#[test]
fn both_tabs_name_this_module_and_keep_their_page() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(
        manifest["schema"], 1,
        "a kit that opens on Hangar 1.x stays at schema 1"
    );
    for tab in manifest["tabs"].as_array().unwrap() {
        assert_eq!(tab["native"], "tabs/github.wasm");
        assert!(tab["entry"].as_str().unwrap().ends_with(".html"));
    }
    assert!(MODULE.len() < 4 * 1024 * 1024, "{} bytes", MODULE.len());
}
