import { expect, test } from 'bun:test'

test('Regression: invalid TS segfaulted bun test; CI typechecks first and rejects conflict markers', async () => {
  const yaml = await Bun.file(new URL('../.github/workflows/ci.yml', import.meta.url)).text()
  const appJob = yaml.split(/\n  app:/)[1] ?? ''
  expect(appJob.length).toBeGreaterThan(0)
  const typecheck = appJob.indexOf('bun run typecheck')
  const bunTest = appJob.indexOf('bun test')
  expect(typecheck).toBeGreaterThan(-1)
  expect(bunTest).toBeGreaterThan(-1)
  expect(typecheck).toBeLessThan(bunTest)
  expect(yaml).toContain("git grep -nE '^<<<<<<<'")
})
