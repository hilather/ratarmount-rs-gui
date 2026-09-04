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

export function changeByTestId(node: ReactNode, testId: string, value: string): void {
  const el = getByTestId(node, testId)
  const onChange = (el.props as { onChange?: (e: { value: string }) => void }).onChange
  if (!onChange) {
    throw new Error(`No onChange handler on testId="${testId}"`)
  }
  onChange({ value })
}

export function clickByTestId(node: ReactNode, testId: string, event: object = {}): void {
  const el = getByTestId(node, testId)
  const onClick = (el.props as { onClick?: (e: object) => void }).onClick
  if (!onClick) {
    throw new Error(`No onClick handler on testId="${testId}"`)
  }
  onClick(event)
}

export function keyDownByTestId(node: ReactNode, testId: string, key: string): void {
  const el = getByTestId(node, testId)
  const onKeyDown = (el.props as { onKeyDown?: (e: { key: string }) => void }).onKeyDown
  if (!onKeyDown) {
    throw new Error(`No onKeyDown handler on testId="${testId}"`)
  }
  onKeyDown({ key })
}

export function dropByTestId(node: ReactNode, testId: string, path: string): void {
  const el = getByTestId(node, testId)
  const onDrop = (el.props as { onDrop?: (e: { value: string }) => void }).onDrop
  if (!onDrop) {
    throw new Error(`No onDrop handler on testId="${testId}"`)
  }
  onDrop({ value: path })
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
