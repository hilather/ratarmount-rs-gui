import { expect, test } from 'bun:test'

import { PLACEHOLDER, WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

test('hello window is titled ratarmount at 1100x720', () => {
  expect(WINDOW_TITLE).toBe('ratarmount')
  expect(WINDOW_WIDTH).toBe(1100)
  expect(WINDOW_HEIGHT).toBe(720)
  expect(PLACEHOLDER).toBe('Open an archive')
})

test('package scripts do not advertise a browser target', async () => {
  const pkg = (await Bun.file(new URL('./package.json', import.meta.url)).json()) as {
    scripts: Record<string, string>
  }
  expect(pkg.scripts.dev).toBe('bun --hot app.tsx')
  expect(pkg.scripts.web).toBeUndefined()
  expect(pkg.scripts['web:dev']).toBeUndefined()
})
