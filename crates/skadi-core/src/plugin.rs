//! Korumalı WASM eklentileri. Bir eklenti yalnızca tweak paketi metni üretebilir:
//!
//! - Hiçbir ana makine fonksiyonu verilmez (dosya, ağ, saat, komut yok); bir şey içe aktaran
//!   eklenti yüklenmez.
//! - Çalışma süresi yakıtla, bellek 16 MiB ile, çıktı 512 KiB ile sınırlıdır.
//! - Çıktı imzalı tweak paketleriyle aynı doğrulamadan geçer; eklenti dosyasının kendisi de imzalı
//!   olmalıdır.
//!
//! ABI: eklenti `memory` belleğini ve `skadi_tweaks() -> i64` fonksiyonunu dışa aktarır; dönüş
//! değerinin üst 32 biti metnin bellekteki adresi, alt 32 biti uzunluğudur.

use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::error::{Error, Result};

pub const MAX_PLUGIN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const FUEL: u64 = 50_000_000;
const MEMORY_BYTES: usize = 16 * 1024 * 1024;

fn fail(detail: impl std::fmt::Display) -> Error {
    Error::Catalog(format!("plugin: {detail}"))
}

/// Eklentiyi çalıştırır ve ürettiği tweak paketi metnini döndürür.
pub fn run(wasm: &[u8]) -> Result<String> {
    if wasm.len() > MAX_PLUGIN_BYTES {
        return Err(fail("file is too large"));
    }
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, wasm).map_err(fail)?;
    if let Some(import) = module.imports().next() {
        return Err(fail(format!(
            "plugins may not import anything (asked for {}.{})",
            import.module(),
            import.name()
        )));
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(MEMORY_BYTES)
        .instances(1)
        .memories(1)
        .tables(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(FUEL).map_err(fail)?;
    let linker = Linker::<StoreLimits>::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .and_then(|pre| pre.start(&mut store))
        .map_err(fail)?;
    let entry = instance
        .get_typed_func::<(), i64>(&store, "skadi_tweaks")
        .map_err(fail)?;
    let packed = entry.call(&mut store, ()).map_err(fail)? as u64;
    let (offset, len) = ((packed >> 32) as usize, (packed & 0xFFFF_FFFF) as usize);
    if len > MAX_OUTPUT_BYTES {
        return Err(fail("output is too large"));
    }
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| fail("no exported memory"))?;
    let mut buffer = vec![0u8; len];
    memory.read(&store, offset, &mut buffer).map_err(fail)?;
    String::from_utf8(buffer).map_err(|_| fail("output is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(body: &str) -> Vec<u8> {
        wat::parse_str(body).unwrap()
    }

    #[test]
    fn returns_text_from_memory() {
        let toml = "[[tweak]]\\nid = \\\"plugin-x\\\"\\n";
        let len = "[[tweak]]\nid = \"plugin-x\"\n".len();
        let wasm = module(&format!(
            r#"(module
                 (memory (export "memory") 1)
                 (data (i32.const 16) "{toml}")
                 (func (export "skadi_tweaks") (result i64)
                   (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const {len}))))"#
        ));
        assert_eq!(run(&wasm).unwrap(), "[[tweak]]\nid = \"plugin-x\"\n");
    }

    #[test]
    fn rejects_imports_and_endless_loops() {
        let importing = module(
            r#"(module
                 (import "env" "open" (func))
                 (memory (export "memory") 1)
                 (func (export "skadi_tweaks") (result i64) (i64.const 0)))"#,
        );
        assert!(
            run(&importing)
                .unwrap_err()
                .message(crate::Lang::En)
                .contains("may not import")
        );
        let endless = module(
            r#"(module
                 (memory (export "memory") 1)
                 (func (export "skadi_tweaks") (result i64) (loop $l (br $l)) (i64.const 0)))"#,
        );
        assert!(run(&endless).is_err(), "fuel must stop an endless loop");
        let greedy = module(
            r#"(module
                 (memory (export "memory") 1)
                 (func (export "skadi_tweaks") (result i64)
                   (drop (memory.grow (i32.const 1000)))
                   (i64.const 0)))"#,
        );
        // Bellek 16 MiB'ın üstüne büyütülemez; büyütme başarısız olur ama eklenti çökmeden biter.
        assert_eq!(run(&greedy).unwrap(), "");
    }
}
