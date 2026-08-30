import { wrapNativeModule, type NativeAddon } from './napi'

/**
 * How the UI imports the W1 napi addon.
 *
 * After `cd native && bun install && bun run build` (enables Cargo feature
 * `napi-addon`; or `napi build --platform --esm --features napi-addon` in
 * `native/`), W3 loads commands from the generated binding:
 *
 *   import { pickFile, list, open, close, on } from '../native'
 *
 * `bun run dev` still starts if the `.node` is missing; Open then surfaces
 * the load error. Listing is paged with an opaque cursor string.
 * There is no `readAll`. Archive member bytes never cross this import.
 */
export const NATIVE_ADDON_MODULE = '../native' as const

export async function loadNativeAddon(): Promise<NativeAddon> {
  const spec: string = NATIVE_ADDON_MODULE
  try {
    const mod: unknown = await import(spec)
    return wrapNativeModule(mod)
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err)
    throw new Error(
      `Native addon is not built (${detail}). From native/: bun install && bun run build`,
    )
  }
}
