# Capsule Bundle Format (M15, scope acotado)

Formato binario mínimo para empaquetar manifest + payload de guest.

## Layout (little-endian)
- bytes 0..8: magic `CAPSULE\0`
- u32: version (`1`)
- u32: compression (`0 = none`)
- u64: manifest length
- u64: payload stored length
- u64: payload uncompressed length
- manifest bytes: UTF-8 key=value lines (`name`, `guest_os`, `backend`, `memory_mib`, `vcpus`)
- payload bytes

## Estado M15
- Implementado: writer/reader, validación de magic/version/compression, roundtrip determinista, CLI `test-bundle`.
- No implementado: compresión real, firma/crypto, streaming por bloques, integración con PE/Single-EXE, carga directa de guest desde bundle.
- `M16/M17` se basarán en este formato, pero no están declarados soportados.
