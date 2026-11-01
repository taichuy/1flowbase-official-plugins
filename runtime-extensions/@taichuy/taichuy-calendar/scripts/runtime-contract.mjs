// Shared public DTO declarations. Consumed by the manifest and bundled backend validator.
export const object = (properties, required = Object.keys(properties)) => ({ type: 'object', properties, required, additionalProperties: false });
const integer = (minimum, maximum) => ({ type: 'integer', minimum, maximum });
const number = (minimum, maximum) => ({ type: 'number', minimum, maximum });
export const civilDate = { type: 'string', pattern: '^[+-]?[0-9]{4,6}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$' };
export const civilTime = { type: 'string', pattern: '^([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]$' };
const locationCodes = { type: 'array', items: { type: 'string', pattern: '^[0-9]{2}([0-9]{2})?([0-9]{2})?$' }, minItems: 0, maxItems: 3 };
const birthFields = { civil_date: civilDate, civil_time: civilTime, utc_offset_minutes: integer(-720, 840), gender: { type: 'string', enum: ['male', 'female'] }, location_codes: locationCodes };
const solarFields = { civil_date: civilDate, civil_time: civilTime, utc_offset_minutes: integer(-720, 840), longitude: number(-180, 180), latitude: number(-90, 90) };
export const operations = [
  { name: 'calendar.month', title: '月历 / Month calendar', body: object({ year: { type: 'integer' }, month: integer(1, 12) }) },
  { name: 'calendar.convert', title: '公农历转换 / Calendar conversion', body: object({ calendar: { type: 'string', enum: ['solar', 'lunar'] }, year: { type: 'integer' }, month: integer(1, 12), day: integer(1, 31), is_leap_month: { type: 'boolean' } }, ['calendar', 'year', 'month', 'day']) },
  { name: 'almanac.day', title: '黄历 / Daily almanac', body: object({ civil_date: civilDate }) },
  { name: 'birth.normalize', title: '出生信息规范化 / Normalize birth', body: object(birthFields) },
  { name: 'time.true-solar', title: '真太阳时 / True solar time', body: object(solarFields) },
  { name: 'bazi.calculate', title: '生辰八字 / Bazi chart', body: object({ ...birthFields, as_of: civilDate }) },
  { name: 'ziwei.calculate', title: '紫微斗数 / Ziwei chart', body: object({ ...birthFields, as_of: civilDate, as_of_time: civilTime }) },
  { name: 'regions.children', title: '出生地行政区 / Region children', body: object({ parent_codes: locationCodes }) },
];
export function operationId(name) { return `taichuy-calendar.${name}`; }
export function route(name) { return `/api/console/managed-services/taichuy-calendar/${name.replaceAll('.', '/')}`; }
export const errorSchema = object({ code: { type: 'string', enum: ['invalid_input', 'invalid_date', 'invalid_region', 'unsupported_operation', 'calculation_failed'] }, message: { type: 'string', minLength: 1 } });
export function outputSchema(data) { return { oneOf: [object({ ok: { const: true }, data }), object({ ok: { const: false }, error: errorSchema })] }; }
