import { c, cB } from '../../../_utils/cssr'

// vars:
// --n-bezier
// --n-bezier-ease-out
// --n-text-color
// --n-color
// --n-border
//
// private-vars:
// --n-border-color-xxx, used for custom color
export default c([
  cB(
    'button',
    `
    color: var(--n-text-color);
    background: var(--n-color);
    border: var(--n-border);
    transition: color .3s var(--n-bezier);
  `
  )
])
