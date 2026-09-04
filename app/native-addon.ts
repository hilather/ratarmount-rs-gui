import { wrapNativeModule, type NativeAddon } from './napi'

/** Specifier for the generated napi binding. Loaded dynamically so a missing `.node` does not fail module load. */
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
