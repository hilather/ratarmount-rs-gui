import { render } from '@gpuix/react'
import { useEffect, useMemo, useSyncExternalStore } from 'react'

import { ExplorerController } from './explorer'
import { ExplorerView, explorerHandlers } from './explorer-view'
import { loadNativeAddon } from './native-addon'
import type { NativeAddon } from './napi'
import { WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

export function App({ native }: { native?: NativeAddon } = {}) {
  const controller = useMemo(() => new ExplorerController(), [])
  const model = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const handlers = useMemo(() => explorerHandlers(controller), [controller])

  useEffect(() => {
    if (native) {
      controller.setNative(native)
      return () => controller.dispose()
    }
    controller.setNativeLoader(loadNativeAddon)
    let cancelled = false
    loadNativeAddon()
      .then((addon) => {
        if (!cancelled) {
          controller.setNative(addon)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          controller.failLoad(err)
        }
      })
    return () => {
      cancelled = true
      controller.dispose()
    }
  }, [controller, native])

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
