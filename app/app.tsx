import { render } from '@gpuix/react'

import { PLACEHOLDER, WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'
// Native napi addon (W1): after `cd native && bun run build`, W3 imports
//   import { pickFile, list, open, close, on } from '../native'
// See native-addon.ts. Do not load archive member bytes through this addon.

const CANVAS = '#1A1A1A'
const TEXT = '#E2E2E2'

export function App() {
  return (
    <div
      testId="hello"
      style={{
        width: '100%',
        height: '100%',
        backgroundColor: CANVAS,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <text testId="placeholder" style={{ color: TEXT, fontSize: 16 }}>
        {PLACEHOLDER}
      </text>
    </div>
  )
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
