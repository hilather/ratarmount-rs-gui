import { expect, test } from 'bun:test'

import {
  extractHereDest,
  isHeadlessLaunch,
  launchArgsFromProcess,
  parseArgvVectors,
  parseLaunchArgv,
} from './argv'

test('Regression: ExtractTo -- archive.tar does not interpret the archive as destDir', () => {
  const intent = parseLaunchArgv(['--extract-to', '--', '/tmp/archive.tar'])
  expect(intent.action).toEqual({ kind: 'extract-to', destDir: null })
  expect(intent.archives).toEqual(['/tmp/archive.tar'])
  expect(intent.action.kind === 'extract-to' && intent.action.destDir).not.toBe('/tmp/archive.tar')
})

test('extract-to with a single positional is the archive', () => {
  const intent = parseLaunchArgv(['--extract-to', 'archive.tar'])
  expect(intent.action).toEqual({ kind: 'extract-to', destDir: null })
  expect(intent.archives).toEqual(['archive.tar'])
})

test('extract-to dir then archive', () => {
  const intent = parseLaunchArgv(['--extract-to', '/out', 'archive.tar'])
  expect(intent.action).toEqual({ kind: 'extract-to', destDir: '/out' })
  expect(intent.archives).toEqual(['archive.tar'])
})

test('open extract-here index-only silent', () => {
  expect(parseLaunchArgv(['a.tar']).action.kind).toBe('open')
  expect(parseLaunchArgv(['--extract-here', 'a.tar', 'b.tar']).archives).toEqual(['a.tar', 'b.tar'])
  const silent = parseLaunchArgv(['--silent', '--extract-here', 'a.tar'])
  expect(silent.silent).toBe(true)
  expect(isHeadlessLaunch(silent)).toBe(true)
  expect(parseLaunchArgv(['--index-only', 'a.tar']).action.kind).toBe('index-only')
  expect(isHeadlessLaunch(parseLaunchArgv(['--index-only', 'a.tar']))).toBe(true)
})

test('launchArgsFromProcess drops the bun script', () => {
  expect(launchArgsFromProcess(['bun', '/app/app.tsx', 'hello.tar'])).toEqual(['hello.tar'])
  expect(launchArgsFromProcess(['ratarmount-gui', 'hello.tar'])).toEqual(['hello.tar'])
  expect(extractHereDest('/data/archives/hello.tar')).toBe('/data/archives')
})

test('golden argv vectors match JS and destDir is never the archive', async () => {
  const text = await Bun.file(new URL('../native/tests/argv-vectors.txt', import.meta.url)).text()
  const vectors = parseArgvVectors(text)
  expect(vectors.length).toBeGreaterThan(0)
  for (const v of vectors) {
    const intent = parseLaunchArgv(v.args)
    expect(intent.action.kind).toBe(v.action)
    expect(intent.archives).toEqual(v.archives)
    expect(intent.silent).toBe(v.silent)
    const destDir = intent.action.kind === 'extract-to' ? intent.action.destDir : null
    expect(destDir).toBe(v.destDir)
    if (v.action === 'extract-to') {
      expect(destDir).not.toBe(v.archives[0])
    }
  }
})
