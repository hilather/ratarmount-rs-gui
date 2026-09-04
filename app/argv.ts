import { CommandError } from './napi'

export type LaunchAction =
  | { kind: 'open' }
  | { kind: 'extract-here' }
  | { kind: 'extract-to'; destDir: string | null }
  | { kind: 'index-only' }

export type LaunchIntent = {
  action: LaunchAction
  archives: string[]
  silent: boolean
}

type ActionKind = 'open' | 'extract-here' | 'extract-to' | 'index-only'

export function launchArgsFromProcess(argv: string[]): string[] {
  const rest = argv.slice(1)
  const first = rest[0]
  if (first == null) {
    return []
  }
  const base = first.replaceAll('\\', '/').split('/').pop() ?? ''
  if (base.endsWith('.tsx') || base.endsWith('.ts') || base.endsWith('.js') || base.endsWith('.mjs')) {
    return rest.slice(1)
  }
  return rest
}

export function parseLaunchArgv(args: string[]): LaunchIntent {
  let silent = false
  let action: ActionKind | null = null
  let extractToOmitted = false
  const positionals: string[] = []
  let i = 0
  while (i < args.length) {
    const a = args[i] ?? ''
    if (a === '--silent') {
      silent = true
    } else if (a === '--extract-here') {
      action = setAction(action, 'extract-here')
    } else if (a === '--index-only') {
      action = setAction(action, 'index-only')
    } else if (a === '--extract-to') {
      action = setAction(action, 'extract-to')
      const next = args[i + 1]
      if (next == null || next === '--') {
        extractToOmitted = true
        if (next === '--') {
          i += 1
        }
      } else if (next.startsWith('--')) {
        extractToOmitted = true
      }
    } else if (a === '--') {
      positionals.push(...args.slice(i + 1))
      break
    } else if (a.startsWith('-') && a !== '-') {
      throw new CommandError('Internal', `unknown option '${a}'`, false)
    } else {
      positionals.push(a)
    }
    i += 1
  }

  let resolved: LaunchAction
  if (action === 'extract-here') {
    resolved = { kind: 'extract-here' }
  } else if (action === 'index-only') {
    resolved = { kind: 'index-only' }
  } else if (action === 'extract-to') {
    // One remaining path is the archive, never destDir (`--extract-to -- "%1"`).
    if (extractToOmitted || positionals.length <= 1) {
      resolved = { kind: 'extract-to', destDir: null }
    } else {
      const destDir = positionals.shift() ?? null
      resolved = { kind: 'extract-to', destDir }
    }
  } else {
    resolved = { kind: 'open' }
  }
  return { action: resolved, archives: positionals, silent }
}

function setAction(current: ActionKind | null, next: ActionKind): ActionKind {
  if (current == null || current === 'open' || current === next) {
    return next
  }
  throw new CommandError('Internal', `conflicting actions ${current} and ${next}`, false)
}

export function extractHereDest(archive: string): string {
  const norm = archive.replaceAll('\\', '/')
  const idx = norm.lastIndexOf('/')
  if (idx <= 0) {
    return '.'
  }
  return archive.slice(0, idx)
}

export function isHeadlessLaunch(intent: LaunchIntent): boolean {
  return isHeadlessAction(intent.action.kind, intent.silent)
}

export function isHeadlessAction(action: string, silent: boolean): boolean {
  if (action === 'index-only') {
    return true
  }
  if (!silent) {
    return false
  }
  return action === 'extract-here' || action === 'extract-to'
}

export type ArgvVector = {
  args: string[]
  action: 'open' | 'extract-here' | 'extract-to' | 'index-only'
  destDir: string | null
  archives: string[]
  silent: boolean
}

export function parseArgvVectors(text: string): ArgvVector[] {
  const out: ArgvVector[] = []
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (line === '' || line.startsWith('#')) {
      continue
    }
    const cols = raw.split('\t')
    if (cols.length < 5) {
      throw new CommandError('Internal', `invalid argv vector: ${line}`, false)
    }
    const dest = cols[2] ?? ''
    const actionRaw = cols[1] ?? ''
    const action: ArgvVector['action'] =
      actionRaw === 'extract-here' || actionRaw === 'extract-to' || actionRaw === 'index-only'
        ? actionRaw
        : 'open'
    out.push({
      args: (cols[0] ?? '').split('|').filter(Boolean),
      action,
      destDir: dest === '' ? null : dest,
      archives: (cols[3] ?? '').split('|').filter(Boolean),
      silent: cols[4]?.trim() === '1',
    })
  }
  return out
}
