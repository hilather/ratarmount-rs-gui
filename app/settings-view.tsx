import type { ReactNode } from 'react'

import {
  CACHE_CAP_PRESETS_GIB,
  PERSISTABLE_POLICIES,
  PREVIEW_CAP_PRESETS_MIB,
  RECREATE_OPTIONS,
  SettingsController,
  TEMP_POLICY_WARNING,
  bytesToMib,
  gibToBytes,
  mibToBytes,
  type SettingsSnapshot,
} from './settings'
import type { PersistablePolicy, Recreate } from './napi'

const CANVAS = '#1A1A1A'
const PANEL = '#202020'
const BORDER = '#333333'
const TEXT = '#E2E2E2'
const MUTED = '#9A9A9A'
const ACCENT = '#2A3F54'
const BUTTON = '#2C2C2C'
const BUTTON_HOVER = '#3A3A3A'
const DANGER = '#E07070'
const WARN = '#D8B060'

export type SettingsHandlers = {
  onBack(): void
  onPolicy(policy: PersistablePolicy): void
  onRecreate(recreate: Recreate): void
  onPreviewMib(mib: number): void
  onCacheGib(gib: number): void
  onAddExtraDir(): void
  onRemoveExtraDir(index: number): void
  onPickExplicit(): void
  onClearCache(): void
  onRegisterAssociations(): void
  onUnregisterAssociations(): void
  onToggleUnsafePaths(): void
}

export function settingsHandlers(controller: SettingsController, onBack: () => void): SettingsHandlers {
  return {
    onBack,
    onPolicy: (policy) => {
      void controller.setPolicy(policy)
    },
    onRecreate: (recreate) => {
      void controller.setRecreate(recreate)
    },
    onPreviewMib: (mib) => {
      void controller.setPreviewMaxBytes(mibToBytes(mib))
    },
    onCacheGib: (gib) => {
      void controller.setLocalCacheBytes(gibToBytes(gib))
    },
    onAddExtraDir: () => {
      void controller.addExtraDir()
    },
    onRemoveExtraDir: (index) => {
      void controller.removeExtraDir(index)
    },
    onPickExplicit: () => {
      void controller.pickExplicitPath()
    },
    onClearCache: () => {
      void controller.clearLocalIndexCache()
    },
    onRegisterAssociations: () => {
      void controller.registerAssociations()
    },
    onUnregisterAssociations: () => {
      void controller.unregisterAssociations()
    },
    onToggleUnsafePaths: () => {
      void controller.toggleUnsafePaths()
    },
  }
}

export function SettingsView({
  model,
  onBack,
  onPolicy,
  onRecreate,
  onPreviewMib,
  onCacheGib,
  onAddExtraDir,
  onRemoveExtraDir,
  onPickExplicit,
  onClearCache,
  onRegisterAssociations,
  onUnregisterAssociations,
  onToggleUnsafePaths,
}: { model: SettingsSnapshot } & SettingsHandlers) {
  const previewMib = bytesToMib(model.previewMaxBytes)
  const cacheGib = model.localCacheBytes / (1024 * 1024 * 1024)

  return (
    <div
      testId="settings"
      style={{
        width: '100%',
        height: '100%',
        backgroundColor: CANVAS,
        flexDirection: 'column',
        userSelect: 'none',
      }}
    >
      <div
        testId="settings-toolbar"
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 8,
          paddingBottom: 8,
          backgroundColor: PANEL,
          borderBottomWidth: 1,
          borderColor: BORDER,
        }}
      >
        <Chip testId="settings-back" label="Back" active={false} onClick={onBack} />
        <text style={{ color: TEXT, fontSize: 14, flexGrow: 1 }}>Settings</text>
      </div>

      <div
        testId="settings-body"
        style={{
          flexGrow: 1,
          minHeight: 0,
          paddingLeft: 16,
          paddingRight: 16,
          paddingTop: 16,
          paddingBottom: 16,
          flexDirection: 'column',
          gap: 16,
        }}
      >
        <Section title="Index policy">
          <div style={{ flexDirection: 'row', gap: 8 }}>
            {PERSISTABLE_POLICIES.map((policy) => (
              <Chip
                key={policy}
                testId={`policy-${policy}`}
                label={policy}
                active={model.policy === policy}
                onClick={() => onPolicy(policy)}
              />
            ))}
          </div>
          {model.policy === 'temp' ? (
            <text testId="temp-warning" style={{ color: WARN, fontSize: 12 }}>
              {TEMP_POLICY_WARNING}
            </text>
          ) : null}
          {model.policy === 'explicit' ? (
            <div style={{ flexDirection: 'row', gap: 8, alignItems: 'center' }}>
              <text testId="explicit-path" style={{ color: MUTED, fontSize: 12, flexGrow: 1 }}>
                {model.explicitPath || 'No explicit index path'}
              </text>
              <Chip testId="explicit-pick" label="Choose file" active={false} onClick={onPickExplicit} />
            </div>
          ) : null}
        </Section>

        <Section title="Recreate index">
          <div style={{ flexDirection: 'row', gap: 8 }}>
            {RECREATE_OPTIONS.map((recreate) => (
              <Chip
                key={recreate}
                testId={`recreate-${recreate}`}
                label={recreate}
                active={model.recreate === recreate}
                onClick={() => onRecreate(recreate)}
              />
            ))}
          </div>
        </Section>

        <Section title="Preview cap">
          <div style={{ flexDirection: 'row', gap: 8 }}>
            {PREVIEW_CAP_PRESETS_MIB.map((mib) => (
              <Chip
                key={mib}
                testId={`preview-cap-${mib}`}
                label={`${mib} MiB`}
                active={previewMib === mib}
                onClick={() => onPreviewMib(mib)}
              />
            ))}
          </div>
          <text testId="preview-cap-value" style={{ color: MUTED, fontSize: 12 }}>
            {`${model.previewMaxBytes} bytes (native clamps at 64 MiB)`}
          </text>
        </Section>

        <Section title="Extra index dirs">
          {model.extraDirs.length === 0 ? (
            <text testId="extra-dirs-empty" style={{ color: MUTED, fontSize: 12 }}>
              None
            </text>
          ) : (
            model.extraDirs.map((dir, index) => (
              <div
                key={`${dir}-${index}`}
                testId={`extra-dir-${index}`}
                style={{ flexDirection: 'row', gap: 8, alignItems: 'center' }}
              >
                <text style={{ color: TEXT, fontSize: 12, flexGrow: 1 }}>{dir}</text>
                <Chip
                  testId={`extra-dir-remove-${index}`}
                  label="Remove"
                  active={false}
                  onClick={() => onRemoveExtraDir(index)}
                />
              </div>
            ))
          )}
          <Chip testId="extra-dir-add" label="Add folder" active={false} onClick={onAddExtraDir} />
        </Section>

        <Section title="Local index cache">
          <div style={{ flexDirection: 'row', gap: 8 }}>
            {CACHE_CAP_PRESETS_GIB.map((gib) => (
              <Chip
                key={gib}
                testId={`cache-cap-${gib}`}
                label={gib < 1 ? `${gib * 1024} MiB` : `${gib} GiB`}
                active={cacheGib === gib}
                onClick={() => onCacheGib(gib)}
              />
            ))}
          </div>
          <Chip
            testId="clear-cache"
            label="Clear local index cache"
            active={false}
            onClick={onClearCache}
          />
          {model.cacheRemoved != null ? (
            <text testId="cache-cleared" style={{ color: MUTED, fontSize: 12 }}>
              {`Removed ${model.cacheRemoved} from local-index-v1`}
            </text>
          ) : (
            <text style={{ color: MUTED, fontSize: 12 }}>
              Wipes local-index-v1 only. Sibling sidecars and the legacy CLI cache stay.
            </text>
          )}
        </Section>

        <Section title="File associations">
          <text style={{ color: MUTED, fontSize: 12 }}>
            Become default handler for TAR/ZIP/7z (best-effort).
          </text>
          <div style={{ flexDirection: 'row', gap: 8 }}>
            <Chip
              testId="settings-register"
              label="Register file associations"
              active={false}
              onClick={onRegisterAssociations}
            />
            <Chip
              testId="settings-unregister"
              label="Unregister"
              active={false}
              onClick={onUnregisterAssociations}
            />
          </div>
          <Chip
            testId="settings-unsafe-paths"
            label={model.allowUnsafePaths ? 'Allow unsafe paths: on' : 'Allow unsafe paths: off'}
            active={model.allowUnsafePaths}
            onClick={onToggleUnsafePaths}
          />
        </Section>

        {model.error ? (
          <text testId="settings-error" style={{ color: DANGER, fontSize: 13 }}>
            {model.error}
          </text>
        ) : null}
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div style={{ flexDirection: 'column', gap: 8 }}>
      <text style={{ color: MUTED, fontSize: 11 }}>{title}</text>
      {children}
    </div>
  )
}

function Chip({
  testId,
  label,
  active,
  onClick,
}: {
  testId: string
  label: string
  active: boolean
  onClick: () => void
}) {
  return (
    <div
      testId={testId}
      onClick={onClick}
      style={{
        paddingLeft: 10,
        paddingRight: 10,
        paddingTop: 6,
        paddingBottom: 6,
        backgroundColor: active ? ACCENT : BUTTON,
        borderRadius: 4,
        cursor: 'pointer',
        hover: { backgroundColor: active ? ACCENT : BUTTON_HOVER },
      }}
    >
      <text style={{ color: TEXT, fontSize: 13 }}>{label}</text>
    </div>
  )
}
