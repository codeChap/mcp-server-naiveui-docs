import commonVariables from './_common'

const base = {
  primaryDefault: '#18a058',
  primaryHover: '#36ad6a',
  primaryActive: '#0c7a43'
}

const derived = {
  name: 'common' as const,

  ...commonVariables,

  primaryColor: base.primaryDefault,
  primaryColorHover: base.primaryHover,
  primaryColorPressed: base.primaryActive,

  textColor1: 'rgb(31, 34, 37)',
  textColor2: 'rgb(51, 54, 57)'
}

export default derived
export type ThemeCommonVars = typeof derived
