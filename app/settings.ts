import {
  CommandError,
  commandErrorFromUnknown,
  type Config,
  type ConfigPatch,
  type IndexPolicy,
  type NativeAddon,
  type PersistablePolicy,
  type Recreate,
  PREVIEW_CEILING_BYTES,
  PREVIEW_DEFAULT_BYTES,
} from './napi'

export const PERSISTABLE_POLICIES: readonly PersistablePolicy[] = [
  'sibling',
  'user-cache',
  'explicit',
  'temp',
]

export const RECREATE_OPTIONS: readonly Recreate[] = ['never', 'if-invalid', 'always']

export const PREVIEW_CAP_PRESETS_MIB = [8, 16, 32, 64] as const
export const CACHE_CAP_PRESETS_GIB = [0.5, 1, 2, 8] as const

export const TEMP_POLICY_WARNING =
  'Temp indexes live in a private directory (mode 0700) and are deleted when the session closes. /tmp is often a RAM disk, world-readable, and wiped on reboot — it is not the default.'

export const SIBLING_NOT_WRITABLE_PROMPT = 'Use user cache?'
export const SIBLING_NOT_WRITABLE_DETAIL =
  'The folder next to the archive is not writable. Save the index in the user cache instead?'
export const REMEMBER_VOLUME_LABEL = 'Always for this filesystem'

export function hideMemoryPolicy(policy: IndexPolicy): PersistablePolicy {
  return policy === 'memory' ? 'sibling' : policy
}

export function indexLocationHint(
  policy: IndexPolicy,
  source: string,
  explicitPath?: string,
): string {
  switch (policy) {
    case 'sibling':
      return `${source}.index.sqlite`
    case 'user-cache':
      return 'user cache'
    case 'explicit':
      return explicitPath && explicitPath.length > 0 ? explicitPath : 'explicit'
    case 'temp':
      return 'temp'
    case 'memory':
      return ':memory:'
    default:
      return 'unresolved'
  }
}

/** Match native `volume_key_for_source`: Path parent, empty parent → source. Keep OS separators. */
export function volumeKeyForSource(source: string): string {
  const win = typeof process !== 'undefined' && process.platform === 'win32'
  if (!win) {
    const idx = source.lastIndexOf('/')
    if (idx < 0) {
      return source
    }
    if (idx === 0) {
      return '/'
    }
    return source.slice(0, idx)
  }
  const idx = Math.max(source.lastIndexOf('\\'), source.lastIndexOf('/'))
  if (idx < 0) {
    return source
  }
  const parent = source.slice(0, idx)
  if (parent === '') {
    return source
  }
  // Native Path::parent("C:\\hello.tar") is "C:\\".
  if (/^[A-Za-z]:$/.test(parent)) {
    return source.slice(0, idx + 1)
  }
  return parent
}

export function effectiveOpenPolicy(
  policy: PersistablePolicy,
  source: string,
  rememberedVolumes: readonly string[],
  rememberUnwritableVolumes: boolean,
): PersistablePolicy {
  if (
    policy === 'sibling' &&
    rememberUnwritableVolumes &&
    rememberedVolumes.includes(volumeKeyForSource(source))
  ) {
    return 'user-cache'
  }
  return policy
}

export function policyBadge(policy: IndexPolicy | null): string {
  if (policy == null) {
    return '—'
  }
  return policy === 'memory' ? 'sibling' : policy
}

export function bytesToMib(bytes: number): number {
  return bytes / (1024 * 1024)
}

export function mibToBytes(mib: number): number {
  return Math.round(mib * 1024 * 1024)
}

export function gibToBytes(gib: number): number {
  return Math.round(gib * 1024 * 1024 * 1024)
}

export type SettingsSnapshot = {
  nativeReady: boolean
  error: string | null
  policy: PersistablePolicy
  recreate: Recreate
  previewMaxBytes: number
  extraDirs: string[]
  localCacheBytes: number
  explicitPath: string
  rememberUnwritableVolumes: boolean
  allowUnsafePaths: boolean
  cacheRemoved: number | null
  saving: boolean
}

const DEFAULT_SNAP: SettingsSnapshot = {
  nativeReady: false,
  error: null,
  policy: 'sibling',
  recreate: 'if-invalid',
  previewMaxBytes: PREVIEW_DEFAULT_BYTES,
  extraDirs: [],
  localCacheBytes: 2 * 1024 * 1024 * 1024,
  explicitPath: '',
  rememberUnwritableVolumes: true,
  allowUnsafePaths: false,
  cacheRemoved: null,
  saving: false,
}

type Listener = () => void

export class SettingsController {
  private native: NativeAddon | null = null
  private snapshot: SettingsSnapshot = { ...DEFAULT_SNAP }
  private readonly listeners = new Set<Listener>()

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getSnapshot = (): SettingsSnapshot => this.snapshot

  setNative(native: NativeAddon): void {
    this.native = native
    this.patch({ nativeReady: true })
    void this.reload()
  }

  failLoad(err: unknown): void {
    const ce = commandErrorFromUnknown(err)
    this.patch({ nativeReady: false, error: ce.message })
  }

  async reload(): Promise<void> {
    const native = this.native
    if (!native) {
      return
    }
    try {
      const cfg = await native.getConfig()
      this.applyConfig(cfg)
    } catch (err) {
      this.setError(err)
    }
  }

  async setPolicy(policy: PersistablePolicy): Promise<void> {
    await this.patchIndex({ policy })
  }

  async setRecreate(recreate: Recreate): Promise<void> {
    await this.patchIndex({ recreate })
  }

  async setPreviewMaxBytes(maxBytes: number): Promise<void> {
    await this.patchConfig({ preview: { maxBytes } })
  }

  async setLocalCacheBytes(localCacheBytes: number): Promise<void> {
    await this.patchIndex({ localCacheBytes })
  }

  async setExplicitPath(explicitPath: string): Promise<void> {
    await this.patchIndex({ explicitPath })
  }

  async pickExplicitPath(): Promise<void> {
    const native = this.requireNative()
    if (!native?.pickFile) {
      return
    }
    const picked = await native.pickFile()
    if (picked != null) {
      await this.setExplicitPath(picked)
    }
  }

  async addExtraDir(): Promise<void> {
    const native = this.requireNative()
    if (!native?.pickDir) {
      return
    }
    const picked = await native.pickDir()
    if (picked == null) {
      return
    }
    if (this.snapshot.extraDirs.includes(picked)) {
      return
    }
    await this.patchIndex({ extraDirs: [...this.snapshot.extraDirs, picked] })
  }

  async removeExtraDir(index: number): Promise<void> {
    const next = this.snapshot.extraDirs.filter((_, i) => i !== index)
    await this.patchIndex({ extraDirs: next })
  }

  async setRememberUnwritableVolumes(value: boolean): Promise<void> {
    await this.patchIndex({ rememberUnwritableVolumes: value })
  }

  async setAllowUnsafePaths(value: boolean): Promise<void> {
    await this.patchConfig({ extract: { allowUnsafePaths: value } })
  }

  async toggleUnsafePaths(): Promise<void> {
    await this.setAllowUnsafePaths(!this.snapshot.allowUnsafePaths)
  }

  async registerAssociations(): Promise<void> {
    const native = this.requireNative()
    if (!native) {
      return
    }
    try {
      await native.registerAssociations()
    } catch (err) {
      this.setError(err)
    }
  }

  async unregisterAssociations(): Promise<void> {
    const native = this.requireNative()
    if (!native) {
      return
    }
    try {
      await native.unregisterAssociations()
    } catch (err) {
      this.setError(err)
    }
  }

  async clearLocalIndexCache(): Promise<void> {
    const native = this.requireNative()
    if (!native?.clearLocalIndexCache) {
      return
    }
    this.patch({ saving: true, error: null })
    try {
      const result = await native.clearLocalIndexCache()
      this.patch({ saving: false, cacheRemoved: result.removed })
    } catch (err) {
      this.patch({ saving: false })
      this.setError(err)
    }
  }

  private async patchIndex(index: NonNullable<ConfigPatch['index']>): Promise<void> {
    await this.patchConfig({ index })
  }

  private async patchConfig(patch: ConfigPatch): Promise<void> {
    const native = this.requireNative()
    if (!native) {
      return
    }
    this.patch({ saving: true, error: null })
    try {
      const cfg = await native.setConfig(patch)
      this.applyConfig(cfg)
      this.patch({ saving: false })
    } catch (err) {
      this.patch({ saving: false })
      this.setError(err)
    }
  }

  private applyConfig(cfg: Config): void {
    const preview = Math.min(cfg.preview.maxBytes, PREVIEW_CEILING_BYTES)
    this.patch({
      policy: hideMemoryPolicy(cfg.index.policy),
      recreate: cfg.index.recreate,
      previewMaxBytes: preview,
      extraDirs: [...cfg.index.extraDirs],
      localCacheBytes: cfg.index.localCacheBytes,
      explicitPath: cfg.index.explicitPath,
      rememberUnwritableVolumes: cfg.index.rememberUnwritableVolumes,
      allowUnsafePaths: cfg.extract.allowUnsafePaths === true,
      error: null,
    })
  }

  private requireNative(): NativeAddon | null {
    if (this.native) {
      return this.native
    }
    this.setError(
      new CommandError(
        'Internal',
        'Native addon is not built. From native/: bun install && bun run build',
        false,
      ),
    )
    return null
  }

  private setError(err: unknown): void {
    const ce = commandErrorFromUnknown(err)
    this.patch({ error: ce.message })
  }

  private patch(partial: Partial<SettingsSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...partial }
    for (const listener of this.listeners) {
      listener()
    }
  }
}
