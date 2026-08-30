import type { EventPayload } from '@gpuix/react'
import type { ReactNode } from 'react'

import {
  countLabel,
  crumbsFor,
  ExplorerController,
  EXTRACT_PLAN_CONFLICT_SAMPLE,
  formatMtime,
  formatSize,
  shortenPath,
  type ClickMods,
  type ExplorerSnapshot,
} from './explorer'
import { PLACEHOLDER } from './window'

const CANVAS = '#1A1A1A'
const PANEL = '#202020'
const BORDER = '#333333'
const TEXT = '#E2E2E2'
const MUTED = '#9A9A9A'
const ACCENT = '#2A3F54'
const BUTTON = '#2C2C2C'
const BUTTON_HOVER = '#3A3A3A'
const DANGER = '#E07070'
const ROW_HEIGHT = 28

export type ExplorerHandlers = {
  onOpen(): void
  onClose(): void
  onExtract(): void
  onCrumb(path: string): void
  onRowClick(index: number, clickCount: number, mods?: ClickMods): void
  onKey(key: string, mods?: ClickMods): void
  onVisibleRange(startIndex: number, endIndex: number): void
  onCancelExtract(): void
  onRetryExtract(): void
  onConfirmExtract(): void
  onOverwriteSkip(): void
  onOverwriteReplace(): void
  onDismissDialog(): void
  onPasswordSubmit(password: string): void
  onExtractOpenSystem(): void
}

function modsFrom(event: EventPayload): ClickMods {
  return {
    shift: event.modifiers?.shift,
    ctrl: event.modifiers?.ctrl,
    cmd: event.modifiers?.cmd,
  }
}

export function explorerHandlers(controller: ExplorerController): ExplorerHandlers {
  return {
    onOpen: () => {
      void controller.openPicked()
    },
    onClose: () => {
      void controller.closeArchive()
    },
    onExtract: () => {
      void controller.extractTo()
    },
    onCrumb: (path) => {
      void controller.enterPath(path)
    },
    onRowClick: (index, clickCount, mods) => {
      controller.onRowClick(index, clickCount, mods)
    },
    onKey: (key, mods) => {
      controller.handleKey(key, mods)
    },
    onVisibleRange: (start, end) => {
      controller.onVisibleRange(start, end)
    },
    onCancelExtract: () => {
      void controller.cancelExtract()
    },
    onRetryExtract: () => {
      void controller.retryExtract()
    },
    onConfirmExtract: () => {
      void controller.confirmExtract()
    },
    onOverwriteSkip: () => {
      void controller.chooseOverwrite('skip')
    },
    onOverwriteReplace: () => {
      void controller.chooseOverwrite('replace')
    },
    onDismissDialog: () => {
      controller.dismissDialog()
    },
    onPasswordSubmit: (password) => {
      void controller.submitPassword(password)
    },
    onExtractOpenSystem: () => {
      void controller.extractOpenWithSystem()
    },
  }
}

export function ExplorerView({
  model,
  onOpen,
  onClose,
  onExtract,
  onCrumb,
  onRowClick,
  onKey,
  onVisibleRange,
  onCancelExtract,
  onRetryExtract,
  onConfirmExtract,
  onOverwriteSkip,
  onOverwriteReplace,
  onDismissDialog,
  onPasswordSubmit,
  onExtractOpenSystem,
}: { model: ExplorerSnapshot } & ExplorerHandlers) {
  const crumbs = crumbsFor(model.path)
  const hasSession = model.archivePath != null && model.status !== 'idle'
  const showBrowser = model.status === 'ready' || (model.status === 'error' && hasSession)

  return (
    <div
      testId="explorer"
      tabIndex={0}
      autoFocus
      onKeyDown={(event: EventPayload) => {
        if (event.isHeld && (event.key === 'enter' || event.key === 'backspace')) {
          return
        }
        if (event.key) {
          onKey(event.key, modsFrom(event))
        }
      }}
      style={{
        width: '100%',
        height: '100%',
        backgroundColor: CANVAS,
        flexDirection: 'column',
        userSelect: 'none',
      }}
    >
      <div
        testId="toolbar"
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
        <ToolButton
          testId="open"
          label="Open"
          onClick={onOpen}
          disabled={!model.nativeReady && model.status !== 'error'}
        />
        <ToolButton
          testId="close"
          label="Close"
          onClick={onClose}
          disabled={!hasSession}
        />
        <ToolButton
          testId="extract"
          label="Extract to…"
          onClick={onExtract}
          disabled={!model.nativeReady || !hasSession || model.status === 'opening'}
        />
      </div>

      {model.status === 'idle' ? (
        <Centered>
          <text testId="placeholder" style={{ color: TEXT, fontSize: 16 }}>
            {PLACEHOLDER}
          </text>
        </Centered>
      ) : null}

      {model.status === 'opening' ? (
        <Centered>
          <text testId="loading" style={{ color: TEXT, fontSize: 16 }}>
            Opening…
          </text>
        </Centered>
      ) : null}

      {model.status === 'error' && !hasSession ? (
        <Centered>
          <text testId="error" style={{ color: DANGER, fontSize: 14 }}>
            {model.error ?? 'Something went wrong'}
          </text>
        </Centered>
      ) : null}

      {showBrowser ? (
        <div style={{ flexGrow: 1, minHeight: 0, flexDirection: 'row' }}>
          <Browser
            model={model}
            crumbs={crumbs}
            onCrumb={onCrumb}
            onRowClick={onRowClick}
            onVisibleRange={onVisibleRange}
          />
          <PreviewPane model={model} onExtractOpenSystem={onExtractOpenSystem} />
        </div>
      ) : null}

      {model.extractJob ? (
        <ProgressPanel
          job={model.extractJob}
          onCancel={onCancelExtract}
          onRetry={onRetryExtract}
        />
      ) : null}

      <div
        testId="status"
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 6,
          paddingBottom: 6,
          backgroundColor: PANEL,
          borderTopWidth: 1,
          borderColor: BORDER,
        }}
      >
        <text testId="status-archive" style={{ color: MUTED, fontSize: 12, flexGrow: 1 }}>
          {model.archivePath ? shortenPath(model.archivePath) : 'No archive'}
        </text>
        <text testId="status-count" style={{ color: MUTED, fontSize: 12 }}>
          {model.status === 'ready'
            ? countLabel(model.totalHint, model.entries.length, model.nextCursor !== null)
            : '—'}
        </text>
        <text testId="status-index" style={{ color: MUTED, fontSize: 12 }}>
          {model.indexPath ? shortenPath(model.indexPath) : '—'}
        </text>
      </div>

      {model.dialog.kind !== 'none' ? (
        <DialogHost
          model={model}
          onConfirmExtract={onConfirmExtract}
          onOverwriteSkip={onOverwriteSkip}
          onOverwriteReplace={onOverwriteReplace}
          onDismissDialog={onDismissDialog}
          onPasswordSubmit={onPasswordSubmit}
        />
      ) : null}
    </div>
  )
}

function Browser({
  model,
  crumbs,
  onCrumb,
  onRowClick,
  onVisibleRange,
}: {
  model: ExplorerSnapshot
  crumbs: ReturnType<typeof crumbsFor>
  onCrumb: (path: string) => void
  onRowClick: (index: number, clickCount: number, mods?: ClickMods) => void
  onVisibleRange: (startIndex: number, endIndex: number) => void
}) {
  return (
    <div style={{ flexGrow: 1, minHeight: 0, flexDirection: 'column' }}>
      <div
        testId="crumbs"
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 4,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 6,
          paddingBottom: 6,
          borderBottomWidth: 1,
          borderColor: BORDER,
        }}
      >
        {crumbs.map((crumb, i) => (
          <div key={crumb.testId} style={{ flexDirection: 'row', alignItems: 'center', gap: 4 }}>
            {i > 0 ? <text style={{ color: MUTED, fontSize: 12 }}>›</text> : null}
            <div
              testId={crumb.testId}
              onClick={() => onCrumb(crumb.path)}
              style={{
                paddingLeft: 4,
                paddingRight: 4,
                paddingTop: 2,
                paddingBottom: 2,
                borderRadius: 3,
                cursor: 'pointer',
                hover: { backgroundColor: BUTTON_HOVER },
              }}
            >
              <text style={{ color: TEXT, fontSize: 12 }}>{crumb.label}</text>
            </div>
          </div>
        ))}
      </div>

      <div
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          paddingLeft: 8,
          paddingRight: 8,
          paddingTop: 4,
          paddingBottom: 4,
          borderBottomWidth: 1,
          borderColor: BORDER,
        }}
      >
        <text style={{ flexGrow: 1, color: MUTED, fontSize: 11, minWidth: 0 }}>Name</text>
        <text style={{ width: 88, color: MUTED, fontSize: 11, textAlign: 'right' }}>Size</text>
        <text style={{ width: 140, color: MUTED, fontSize: 11 }}>Modified</text>
      </div>

      <div testId="list" style={{ flexGrow: 1, minHeight: 0, flexDirection: 'column' }}>
        {model.status === 'error' && model.error ? (
          <Centered>
            <text testId="error" style={{ color: DANGER, fontSize: 14 }}>
              {model.error}
            </text>
          </Centered>
        ) : model.listing && model.entries.length === 0 ? (
          <Centered>
            <text testId="loading" style={{ color: TEXT, fontSize: 14 }}>
              Loading…
            </text>
          </Centered>
        ) : model.entries.length === 0 ? (
          <Centered>
            <text testId="list-empty" style={{ color: MUTED, fontSize: 14 }}>
              This folder is empty
            </text>
          </Centered>
        ) : (
          <virtual-list
            estimatedItemHeight={ROW_HEIGHT}
            onVisibleRange={(event: EventPayload) => {
              onVisibleRange(event.startIndex ?? 0, event.endIndex ?? 0)
            }}
            style={{ flexGrow: 1, minHeight: 0 }}
          >
            {model.entries.map((ent, index) => {
              const focused = index === model.selectedIndex
              const selected = model.selectedPaths.includes(ent.path)
              return (
                <div
                  key={ent.path}
                  testId={`row-${ent.name}`}
                  onClick={(event: EventPayload) =>
                    onRowClick(index, event.clickCount ?? 1, modsFrom(event))
                  }
                  style={{
                    flexDirection: 'row',
                    alignItems: 'center',
                    gap: 8,
                    height: ROW_HEIGHT,
                    paddingLeft: 8,
                    paddingRight: 8,
                    backgroundColor: selected || focused ? ACCENT : 'transparent',
                    hover: { backgroundColor: selected || focused ? ACCENT : '#252525' },
                    cursor: 'pointer',
                  }}
                >
                  <text
                    style={{
                      flexGrow: 1,
                      color: TEXT,
                      fontSize: 13,
                      minWidth: 0,
                      whiteSpace: 'nowrap',
                      textOverflow: 'ellipsis',
                    }}
                  >
                    {ent.isDir ? `${ent.name}/` : ent.name}
                  </text>
                  <text style={{ width: 88, color: MUTED, fontSize: 12, textAlign: 'right' }}>
                    {formatSize(ent)}
                  </text>
                  <text style={{ width: 140, color: MUTED, fontSize: 12 }}>
                    {formatMtime(ent.mtime)}
                  </text>
                </div>
              )
            })}
          </virtual-list>
        )}
      </div>
    </div>
  )
}

function ToolButton({
  testId,
  label,
  onClick,
  disabled = false,
}: {
  testId: string
  label: string
  onClick: () => void
  disabled?: boolean
}) {
  return (
    <div
      testId={testId}
      onClick={() => {
        if (!disabled) {
          onClick()
        }
      }}
      style={{
        paddingLeft: 12,
        paddingRight: 12,
        paddingTop: 6,
        paddingBottom: 6,
        backgroundColor: BUTTON,
        borderRadius: 4,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        ...(disabled ? {} : { hover: { backgroundColor: BUTTON_HOVER } }),
      }}
    >
      <text style={{ color: TEXT, fontSize: 13 }}>{label}</text>
    </div>
  )
}

function PreviewPane({
  model,
  onExtractOpenSystem,
}: {
  model: ExplorerSnapshot
  onExtractOpenSystem: () => void
}) {
  const preview = model.preview
  return (
    <div
      testId="preview"
      style={{
        width: 280,
        borderLeftWidth: 1,
        borderColor: BORDER,
        paddingLeft: 12,
        paddingRight: 12,
        paddingTop: 8,
        paddingBottom: 8,
        flexDirection: 'column',
        gap: 8,
      }}
    >
      <text style={{ color: MUTED, fontSize: 11 }}>Preview</text>
      {preview == null ? (
        <text testId="preview-empty" style={{ color: MUTED, fontSize: 13 }}>
          Select a file
        </text>
      ) : preview.kind === 'text' ? (
        <text testId="preview-text" style={{ color: TEXT, fontSize: 12, whiteSpace: 'nowrap' }}>
          {preview.text}
        </text>
      ) : preview.kind === 'skipped' ? (
        <div style={{ flexDirection: 'column', gap: 8 }}>
          <text testId="preview-skipped" style={{ color: MUTED, fontSize: 13 }}>
            {preview.reason === 'too-large' ? 'Too large to preview' : 'No preview'}
          </text>
          {preview.reason === 'too-large' ? (
            <ToolButton
              testId="extract-open-system"
              label="Extract and open with system"
              onClick={onExtractOpenSystem}
            />
          ) : null}
        </div>
      ) : (
        <text testId="preview-empty" style={{ color: MUTED, fontSize: 13 }}>
          No preview
        </text>
      )}
    </div>
  )
}

function ProgressPanel({
  job,
  onCancel,
  onRetry,
}: {
  job: NonNullable<ExplorerSnapshot['extractJob']>
  onCancel: () => void
  onRetry: () => void
}) {
  const hint = job.filesHint
  const label =
    job.status === 'failed'
      ? job.error ?? 'Extract failed'
      : job.status === 'cancelled'
        ? 'Cancelled'
        : job.status === 'succeeded'
          ? 'Extract complete'
          : hint != null
            ? `${job.filesDone}/${hint}`
            : `${job.filesDone} files`
  return (
    <div
      testId="progress"
      style={{
        flexDirection: 'row',
        alignItems: 'center',
        gap: 8,
        paddingLeft: 12,
        paddingRight: 12,
        paddingTop: 6,
        paddingBottom: 6,
        backgroundColor: PANEL,
        borderTopWidth: 1,
        borderColor: BORDER,
      }}
    >
      <text testId="progress-label" style={{ color: TEXT, fontSize: 12, flexGrow: 1 }}>
        {label}
      </text>
      {job.status === 'running' ? (
        <ToolButton testId="progress-cancel" label="Cancel" onClick={onCancel} />
      ) : null}
      {job.status === 'failed' && job.retryable ? (
        <ToolButton testId="progress-retry" label="Retry" onClick={onRetry} />
      ) : null}
    </div>
  )
}

function DialogHost({
  model,
  onConfirmExtract,
  onOverwriteSkip,
  onOverwriteReplace,
  onDismissDialog,
  onPasswordSubmit,
}: {
  model: ExplorerSnapshot
  onConfirmExtract: () => void
  onOverwriteSkip: () => void
  onOverwriteReplace: () => void
  onDismissDialog: () => void
  onPasswordSubmit: (password: string) => void
}) {
  const dialog = model.dialog
  return (
    <div
      testId="dialog"
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        backgroundColor: '#00000088',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <div
        style={{
          width: 420,
          backgroundColor: PANEL,
          borderWidth: 1,
          borderColor: BORDER,
          borderRadius: 6,
          paddingLeft: 16,
          paddingRight: 16,
          paddingTop: 16,
          paddingBottom: 16,
          flexDirection: 'column',
          gap: 12,
        }}
      >
        {dialog.kind === 'confirm-extract' ? (
          <>
            <text testId="confirm-extract" style={{ color: TEXT, fontSize: 14 }}>
              Extract {dialog.files} files ({formatBytes(dialog.bytes)})?
            </text>
            <div style={{ flexDirection: 'row', gap: 8, justifyContent: 'flex-end' }}>
              <ToolButton testId="confirm-extract-cancel" label="Cancel" onClick={onDismissDialog} />
              <ToolButton testId="confirm-extract-ok" label="Extract" onClick={onConfirmExtract} />
            </div>
          </>
        ) : null}
        {dialog.kind === 'overwrite' ? (
          <>
            <text testId="overwrite-dialog" style={{ color: TEXT, fontSize: 14 }}>
              {dialog.conflictCount} files already exist
            </text>
            {dialog.conflicts.slice(0, EXTRACT_PLAN_CONFLICT_SAMPLE).map((c) => (
              <text key={c.member} style={{ color: MUTED, fontSize: 12 }}>
                {c.member}
              </text>
            ))}
            {dialog.truncated ? (
              <text testId="overwrite-more" style={{ color: MUTED, fontSize: 12 }}>
                and {Math.max(0, dialog.conflictCount - dialog.conflicts.length)} more…
              </text>
            ) : null}
            <div style={{ flexDirection: 'row', gap: 8, justifyContent: 'flex-end' }}>
              <ToolButton testId="overwrite-cancel" label="Cancel" onClick={onDismissDialog} />
              <ToolButton testId="overwrite-skip" label="Skip" onClick={onOverwriteSkip} />
              <ToolButton testId="overwrite-replace" label="Replace" onClick={onOverwriteReplace} />
            </div>
          </>
        ) : null}
        {dialog.kind === 'password' ? (
          <PasswordModal onSubmit={onPasswordSubmit} onCancel={onDismissDialog} />
        ) : null}
        {dialog.kind === 'path-escape' ? (
          <>
            <text testId="path-escape" style={{ color: DANGER, fontSize: 14 }}>
              {dialog.message}
            </text>
            <ToolButton testId="path-escape-ok" label="OK" onClick={onDismissDialog} />
          </>
        ) : null}
      </div>
    </div>
  )
}

const passwordDraft = { current: '' }

function PasswordModal({
  onSubmit,
  onCancel,
}: {
  onSubmit: (password: string) => void
  onCancel: () => void
}) {
  return (
    <div testId="password-modal" style={{ flexDirection: 'column', gap: 12 }}>
      <text style={{ color: TEXT, fontSize: 14 }}>Password required</text>
      <input
        testId="password-input"
        placeholder="Password"
        onChange={(event: EventPayload) => {
          passwordDraft.current = event.value ?? ''
        }}
      />
      <div style={{ flexDirection: 'row', gap: 8, justifyContent: 'flex-end' }}>
        <ToolButton testId="password-cancel" label="Cancel" onClick={onCancel} />
        <div
          testId="password-submit"
          onClick={(event: EventPayload) => {
            const password = event.value ?? passwordDraft.current
            passwordDraft.current = ''
            onSubmit(password)
          }}
          style={{
            paddingLeft: 12,
            paddingRight: 12,
            paddingTop: 6,
            paddingBottom: 6,
            backgroundColor: BUTTON,
            borderRadius: 4,
            cursor: 'pointer',
            hover: { backgroundColor: BUTTON_HOVER },
          }}
        >
          <text style={{ color: TEXT, fontSize: 13 }}>Unlock</text>
        </div>
      </div>
    </div>
  )
}

function formatBytes(n: number): string {
  if (n < 1024) {
    return `${n} B`
  }
  if (n < 1024 * 1024) {
    return `${(n / 1024).toFixed(1)} KiB`
  }
  if (n < 1024 * 1024 * 1024) {
    return `${(n / (1024 * 1024)).toFixed(1)} MiB`
  }
  return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GiB`
}

function Centered({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        flexGrow: 1,
        minHeight: 0,
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      {children}
    </div>
  )
}
