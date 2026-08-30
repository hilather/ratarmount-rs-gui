import { isValidElement, type ReactElement, type ReactNode } from 'react'

/** GPU-free locator over a GPUIX element tree (`testId` props). */
export function getByTestId(node: ReactNode, testId: string): ReactElement {
  const found = queryByTestId(node, testId)
  if (!found) {
    throw new Error(`Unable to find an element with testId="${testId}"`)
  }
  return found
}

export function queryByTestId(node: ReactNode, testId: string): ReactElement | null {
  for (const el of hostElements(node)) {
    const props = el.props as { testId?: string }
    if (props.testId === testId) {
      return el
    }
  }
  return null
}

export function collectTestIds(node: ReactNode): string[] {
  const ids: string[] = []
  for (const el of hostElements(node)) {
    const props = el.props as { testId?: string }
    if (typeof props.testId === 'string') {
      ids.push(props.testId)
    }
  }
  return ids
}

function hostElements(node: ReactNode): ReactElement[] {
  const out: ReactElement[] = []
  walk(node, out)
  return out
}

function walk(node: ReactNode, out: ReactElement[]): void {
  if (node == null || typeof node === 'boolean' || typeof node === 'string' || typeof node === 'number') {
    return
  }
  if (Array.isArray(node)) {
    for (const child of node) {
      walk(child, out)
    }
    return
  }
  if (!isValidElement(node)) {
    return
  }
  const type = node.type
  if (typeof type === 'function') {
    const rendered = (type as (props: object) => ReactNode)(node.props as object)
    walk(rendered, out)
    return
  }
  out.push(node)
  walk((node.props as { children?: ReactNode }).children, out)
}
