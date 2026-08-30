import type { EventPayload } from '@gpuix/react'
import type { ReactNode } from 'react'

import {
  countLabel,
  crumbsFor,
  ExplorerController,
  formatMtime,
  formatSize,
  shortenPath,
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
  onCrumb(path: string): void
  onRowClick(index: number, clickCount: number): void
  onKey(key: string): void
  onVisibleRange(startIndex: number, endIndex: number): void
}

export function explorerHandlers(controller: ExplorerController): ExplorerHandlers {
  return {
    onOpen: () => {
      void controller.openPicked()
    },
    onClose: () => {
      void controller.closeArchive()
    },
    onCrumb: (path) => {
      void controller.enterPath(path)
    },
    onRowClick: (index, clickCount) => {
      controller.onRowClick(index, clickCount)
    },
    onKey: (key) => {
      controller.handleKey(key)
    },
    onVisibleRange: (start, end) => {
      controller.onVisibleRange(start, end)
    },
  }
}

export function ExplorerView({
  model,
  onOpen,
  onClose,
  onCrumb,
  onRowClick,
  onKey,
  onVisibleRange,
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
          onKey(event.key)
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
        <Browser
          model={model}
          crumbs={crumbs}
          onCrumb={onCrumb}
          onRowClick={onRowClick}
          onVisibleRange={onVisibleRange}
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
  onRowClick: (index: number, clickCount: number) => void
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
              const selected = index === model.selectedIndex
              return (
                <div
                  key={ent.path}
                  testId={`row-${ent.name}`}
                  onClick={(event: EventPayload) => onRowClick(index, event.clickCount ?? 1)}
                  style={{
                    flexDirection: 'row',
                    alignItems: 'center',
                    gap: 8,
                    height: ROW_HEIGHT,
                    paddingLeft: 8,
                    paddingRight: 8,
                    backgroundColor: selected ? ACCENT : 'transparent',
                    hover: { backgroundColor: selected ? ACCENT : '#252525' },
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
