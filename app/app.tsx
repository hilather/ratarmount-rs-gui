import { render } from '@gpuix/react'

import { PLACEHOLDER, WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

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

// Tests import `App` without opening a window.
const isEntryPoint =
  typeof Bun !== 'undefined'
    ? Bun.isStandaloneExecutable || Bun.main === import.meta.path
    : typeof window !== 'undefined'

if (isEntryPoint) {
  render(<App />, {
    title: WINDOW_TITLE,
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    // Agent checks need real GPU paint without stealing the user's keyboard.
    focus: typeof process === 'undefined' || process.env.GPUIX_BACKGROUND !== '1',
  })
}
