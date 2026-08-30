import { expect, test } from 'bun:test'

import { clickByTestId, collectTestIds, getByTestId, queryByTestId } from './gpuix-test'
import { createFakeNative } from './fake-native'
import { PREVIEW_CEILING_BYTES } from './napi'
import {
  hideMemoryPolicy,
  indexLocationHint,
  SettingsController,
  TEMP_POLICY_WARNING,
  volumeKeyForSource,
} from './settings'
import { SettingsView, settingsHandlers } from './settings-view'

async function loadSettings() {
  const fake = createFakeNative({ pickDir: '/extra/indexes', pickFile: '/data/idx.sqlite' })
  const controller = new SettingsController()
  controller.setNative(fake)
  await controller.reload()
  return { fake, controller }
}

test('settings hides memory and does not persist it', async () => {
  const { controller, fake } = await loadSettings()
  expect(hideMemoryPolicy('memory')).toBe('sibling')
  const tree = SettingsView({
    model: controller.getSnapshot(),
    ...settingsHandlers(controller, () => {}),
  })
  const ids = collectTestIds(tree)
  expect(ids).toContain('policy-sibling')
  expect(ids).toContain('policy-user-cache')
  expect(ids).toContain('policy-explicit')
  expect(ids).toContain('policy-temp')
  expect(ids).not.toContain('policy-memory')
  expect(queryByTestId(tree, 'policy-memory')).toBeNull()

  try {
    await fake.setConfig({ index: { policy: 'memory' } })
    throw new Error('expected memory reject')
  } catch (err) {
    expect((err as { code?: string }).code).toBe('Internal')
  }
  expect(fake.config.index.policy).toBe('sibling')
})

test('settings policy/recreate/preview/extra dirs/cache cap round-trip on the fake native', async () => {
  const { controller, fake } = await loadSettings()
  await controller.setPolicy('user-cache')
  await controller.setRecreate('always')
  await controller.setPreviewMaxBytes(16 * 1024 * 1024)
  await controller.addExtraDir()
  await controller.setLocalCacheBytes(1 * 1024 * 1024 * 1024)
  expect(fake.config.index.policy).toBe('user-cache')
  expect(fake.config.index.recreate).toBe('always')
  expect(fake.config.preview.maxBytes).toBe(16 * 1024 * 1024)
  expect(fake.config.index.extraDirs).toEqual(['/extra/indexes'])
  expect(fake.config.index.localCacheBytes).toBe(1 * 1024 * 1024 * 1024)

  const tree = SettingsView({
    model: controller.getSnapshot(),
    ...settingsHandlers(controller, () => {}),
  })
  expect(getByTestId(tree, 'policy-user-cache')).toBeTruthy()
  expect(getByTestId(tree, 'recreate-always')).toBeTruthy()
  expect(getByTestId(tree, 'extra-dir-0')).toBeTruthy()
  expect(queryByTestId(tree, 'temp-warning')).toBeNull()
})

test('temp policy shows warning copy', async () => {
  const { controller } = await loadSettings()
  await controller.setPolicy('temp')
  const tree = SettingsView({
    model: controller.getSnapshot(),
    ...settingsHandlers(controller, () => {}),
  })
  const warning = getByTestId(tree, 'temp-warning')
  expect((warning.props as { children?: string }).children).toBe(TEMP_POLICY_WARNING)
  expect(TEMP_POLICY_WARNING).toContain('/tmp')
  expect(TEMP_POLICY_WARNING.toLowerCase()).toContain('not the default')
})

test('preview 65 MiB is clamped by native setConfig', async () => {
  const { controller, fake } = await loadSettings()
  await controller.setPreviewMaxBytes(65 * 1024 * 1024)
  expect(fake.config.preview.maxBytes).toBe(PREVIEW_CEILING_BYTES)
  expect(controller.getSnapshot().previewMaxBytes).toBe(PREVIEW_CEILING_BYTES)
})

test('clear cache button calls clearLocalIndexCache', async () => {
  const { controller, fake } = await loadSettings()
  const tree = SettingsView({
    model: controller.getSnapshot(),
    ...settingsHandlers(controller, () => {}),
  })
  clickByTestId(tree, 'clear-cache')
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(fake.cacheClears).toBe(1)
})

test('index location hint never invents local-index-v1 names', () => {
  expect(indexLocationHint('sibling', '/data/foo.tar')).toBe('/data/foo.tar.index.sqlite')
  expect(indexLocationHint('user-cache', '/data/foo.tar')).toBe('user cache')
  expect(indexLocationHint('user-cache', '/data/foo.tar')).not.toContain('local-index-v1')
  expect(indexLocationHint('temp', '/data/foo.tar')).toBe('temp')
  expect(volumeKeyForSource('/archives/hello.tar')).toBe('/archives')
})

test('settings chips fire policy changes', async () => {
  const { controller, fake } = await loadSettings()
  const tree = SettingsView({
    model: controller.getSnapshot(),
    ...settingsHandlers(controller, () => {}),
  })
  clickByTestId(tree, 'policy-user-cache')
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(fake.config.index.policy).toBe('user-cache')
  expect(controller.getSnapshot().policy).toBe('user-cache')
})
