import { expect, test } from 'vitest'
import { likec4model } from './__generated__/model'

test('generated model resolves likec4/model and has the fixture elements', () => {
  expect([...likec4model.elements()].map(e => e.id).sort()).toEqual(['shop', 'shop.api', 'shop.db', 'user'])
})
