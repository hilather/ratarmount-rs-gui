import { render } from '@gpuix/react'
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'

import { ExplorerController } from './explorer'
import { ExplorerView, explorerHandlers } from './explorer-view'
import { loadNativeAddon } from './native-addon'
import type { NativeAddon } from './napi'
import { SettingsController } from './settings'
import { SettingsView, settingsHandlers } from './settings-view'
import { WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

export function App({ native }: { native?: NativeAddon } = {}) {
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
  const handlers = useMemo(
    () => explorerHandlers(controller, { onSettings: () => setScreen('settings') }),
    [controller],
  )
  const settingHandlers = useMemo(
    () => settingsHandlers(settings, () => setScreen('explorer')),
    [settings],
  )

  useEffect(() => {
    if (native) {
      controller.setNative(native)
      settings.setNative(native)
      return () => controller.dispose()
    }
    controller.setNativeLoader(loadNativeAddon)
    let cancelled = false
    loadNativeAddon()
      .then((addon) => {
        if (!cancelled) {
          controller.setNative(addon)
          settings.setNative(addon)
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
      controller.dispose()
    }
  }, [controller, native, settings])

  if (screen === 'settings') {
    return <SettingsView model={settingsModel} {...settingHandlers} />
  }

  return <ExplorerView model={model} {...handlers} />
}

// Skip `render()` when this module is imported.
if (import.meta.main || Bun.isStandaloneExecutable) {
  render(<App />, {
    title: WINDOW_TITLE,
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    // GPUIX_BACKGROUND=1 opens the window unfocused.
    focus: typeof process === 'undefined' || process.env.GPUIX_BACKGROUND !== '1',
  })
}
