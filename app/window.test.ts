import { expect, test } from 'bun:test'

import { PLACEHOLDER, WINDOW_HEIGHT, WINDOW_TITLE, WINDOW_WIDTH } from './window'

test('hello window is titled ratarmount at 1100x720', () => {
  expect(WINDOW_TITLE).toBe('ratarmount')
  expect(WINDOW_WIDTH).toBe(1100)
  expect(WINDOW_HEIGHT).toBe(720)
  expect(PLACEHOLDER).toBe('Open an archive')
})

test('app.tsx uses the hello-window constants on a dark desktop chrome', async () => {
  const source = await Bun.file(new URL('./app.tsx', import.meta.url)).text()
  expect(source).toContain("from './window'")
  expect(source).toContain('WINDOW_TITLE')
  expect(source).toContain('WINDOW_WIDTH')
  expect(source).toContain('WINDOW_HEIGHT')
  expect(source).toContain('render(')
  expect(source).toContain('<App />')
  expect(source).not.toContain('titlebarTransparent')
  expect(source).not.toContain('typeof window')
})

test('explorer chrome uses the placeholder and dark canvas', async () => {
  const source = await Bun.file(new URL('./explorer-view.tsx', import.meta.url)).text()
  expect(source).toContain('PLACEHOLDER')
  expect(source).toContain('#1A1A1A')
  expect(source).toContain('virtual-list')
  expect(source).toContain('testId="open"')
  expect(source).toContain('testId="list"')
})

test('native addon is loaded with a dynamic specifier, not a static import', async () => {
  const { NATIVE_ADDON_MODULE, loadNativeAddon } = await import('./native-addon')
  expect(NATIVE_ADDON_MODULE).toBe('../native')
  const addonSource = await Bun.file(new URL('./native-addon.ts', import.meta.url)).text()
  const appSource = await Bun.file(new URL('./app.tsx', import.meta.url)).text()
  expect(addonSource).toContain('import(spec)')
  expect(addonSource).not.toMatch(/import\s*\{[^}]*\}\s*from\s*['"]\.\.\/native['"]/)
  expect(appSource).not.toContain("from '../native'")
  try {
    const addon = await loadNativeAddon()
    expect(typeof addon.pickFile).toBe('function')
    expect(typeof addon.list).toBe('function')
  } catch (err) {
    expect(err).toBeInstanceOf(Error)
    expect((err as Error).message).toMatch(/Native addon is not built/)
  }
})

test('package scripts do not advertise a browser target', async () => {
  const here = import.meta.dir
  const pkg = (await Bun.file(`${here}/package.json`).json()) as {
    scripts: Record<string, string>
  }
  expect(pkg.scripts.dev).toBe('bun --hot app.tsx')
  expect(pkg.scripts.web).toBeUndefined()
  expect(pkg.scripts['web:dev']).toBeUndefined()
  expect(pkg.scripts['web:build']).toBeUndefined()
  expect(await Bun.file(`${here}/web.ts`).exists()).toBe(false)
  expect(await Bun.file(`${here}/index.html`).exists()).toBe(false)
})
