import { render } from '@gpuix/react'
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'

import { isHeadlessLaunch, launchArgsFromProcess, parseLaunchArgv } from './argv'
import { bindNativeFileDrop, ExplorerController, gpuixFileDropPath } from './explorer'
import { ExplorerView, explorerHandlers } from './explorer-view'
import { loadNativeAddon } from './native-addon'
import type { NativeAddon } from './napi'
import { SettingsController } from './settings'
import { SettingsView, settingsHandlers } from './settings-view'
import { WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

const dropSink: { open: ((path: string) => void) | null } = { open: null }

export function App({
  native,
  initialArgv,
}: {
  native?: NativeAddon
  initialArgv?: string[]
} = {}) {
  const controller = useMemo(() => new ExplorerController(), [])
  const settings = useMemo(() => new SettingsController(), [])
  const [screen, setScreen] = useState<'explorer' | 'settings'>('explorer')
  const model = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const settingsModel = useSyncExternalStore(
    settings.subscribe,
    settings.getSnapshot,
    settings.getSnapshot,
  )
  const handlers = useMemo(() => {
    const base = explorerHandlers(controller)
    return {
      ...base,
      onSettings: () => setScreen('settings'),
    }
  }, [controller])
  const settingHandlers = useMemo(
    () => settingsHandlers(settings, () => setScreen('explorer')),
    [settings],
  )

  useEffect(() => {
    dropSink.open = (path) => {
      void controller.openDropped(path)
    }
    if (native) {
      controller.setNative(native)
      bindNativeFileDrop(native, controller)
      if (initialArgv && initialArgv.length > 0) {
        void controller.applyArgv(initialArgv)
      }
      settings.setNative(native)
      return () => {
        dropSink.open = null
        controller.dispose()
      }
    }
    controller.setNativeLoader(loadNativeAddon)
    let cancelled = false
    loadNativeAddon()
      .then((addon) => {
        if (cancelled) {
          return
        }
        controller.setNative(addon)
        bindNativeFileDrop(addon, controller)
        settings.setNative(addon)
        const args =
          initialArgv ??
          (typeof process === 'undefined' ? [] : launchArgsFromProcess(process.argv))
        if (args.length > 0) {
          void controller.applyArgv(args)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          controller.failLoad(err)
          settings.failLoad(err)
        }
      })
    return () => {
      cancelled = true
      dropSink.open = null
      controller.dispose()
    }
  }, [controller, native, initialArgv, settings])

  if (screen === 'settings') {
    return <SettingsView model={settingsModel} {...settingHandlers} />
  }

  return <ExplorerView model={model} {...handlers} />
}

async function runSilentLaunch(args: string[]): Promise<void> {
  const addon = await loadNativeAddon()
  await addon.applyLaunch(args)
}

// Skip `render()` when this module is imported.
if (import.meta.main || Bun.isStandaloneExecutable) {
  const args = typeof process === 'undefined' ? [] : launchArgsFromProcess(process.argv)
  let headless = false
  try {
    headless = isHeadlessLaunch(parseLaunchArgv(args))
  } catch {
    headless = false
  }
  if (headless) {
    void runSilentLaunch(args).then(
      () => process.exit(0),
      (err: unknown) => {
        const message = err instanceof Error ? err.message : String(err)
        console.error(message)
        process.exit(1)
      },
    )
  } else {
    render(<App />, {
      title: WINDOW_TITLE,
      width: WINDOW_WIDTH,
      height: WINDOW_HEIGHT,
      // GPUIX_BACKGROUND=1 opens the window unfocused.
      focus: typeof process === 'undefined' || process.env.GPUIX_BACKGROUND !== '1',
      onEvent: (event) => {
        const path = gpuixFileDropPath(event)
        if (path) {
          dropSink.open?.(path)
        }
      },
    })
  }
}
