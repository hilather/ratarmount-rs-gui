/**
 * How the UI imports the W1 napi addon.
 *
 * After `cd native && bun install && bun run build` (enables Cargo feature
 * `napi-addon`; or `napi build --platform --esm --features napi-addon` in
 * `native/`), W3 should load commands from the generated binding:
 *
 *   import { pickFile, list, open, close, on } from '../native'
 *
 * `bun run dev` does not load the addon yet so the hello window still starts
 * without a built `.node`. Listing is paged with an opaque cursor string.
 * There is no `readAll`. Archive member bytes never cross this import.
 */
export const NATIVE_ADDON_MODULE = '../native' as const
