import { operations } from '../../../scripts/runtime-contract.mjs';
import { BusinessError, fail, validate, normalizeBirth, regionPath } from './input';
import { month, convert, almanac } from './calendar';
import { trueSolar, bazi, ziwei } from './charts';
const handlers: Record<string, (input: any) => unknown> = {
  'calendar.month': month, 'calendar.convert': convert, 'almanac.day': almanac,
  'birth.normalize': normalizeBirth, 'time.true-solar': trueSolar, 'bazi.calculate': bazi, 'ziwei.calculate': ziwei,
  'regions.children': input => ({ parent_codes: input.parent_codes, children: regionPath(input.parent_codes).children.map(v => ({ code: v.code, name: v.name, has_children: Boolean(v.children?.length) })) }),
};
export function main(input: { operation: string; body: unknown }) {
  try {
    const operation = operations.find(v => v.name === input.operation);
    if (!operation) fail('unsupported_operation', 'Unsupported calendar operation');
    validate(input.body, operation.body);
    return { ok: true, data: handlers[input.operation](input.body) };
  } catch (error) {
    if (error instanceof BusinessError) return { ok: false, error: { code: error.code, message: error.message } };
    return { ok: false, error: { code: 'calculation_failed', message: 'Calendar calculation failed' } };
  }
}
