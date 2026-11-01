import dayjs from 'dayjs';
import utc from 'dayjs/plugin/utc';
import regions from 'china-division/dist/pca-code.json';
import coordinates from '../../data/coordinates';
dayjs.extend(utc);
export class BusinessError extends Error {
  constructor(public code: string, message: string) { super(message); }
}
export function fail(code: string, message: string): never { throw new BusinessError(code, message); }
export function validate(value: any, schema: any, path = '$'): void {
  const type = schema.type;
  if (schema.enum && !schema.enum.includes(value)) fail('invalid_input', `Invalid input at ${path}`);
  if (type === 'object') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) fail('invalid_input', `Invalid object at ${path}`);
    for (const key of schema.required ?? []) if (!Object.hasOwn(value, key)) fail('invalid_input', `Missing input at ${path}.${key}`);
    for (const key of Object.keys(value)) {
      if (!Object.hasOwn(schema.properties, key)) fail('invalid_input', `Unknown input at ${path}.${key}`);
      validate(value[key], schema.properties[key], `${path}.${key}`);
    }
  } else if (type === 'array') {
    if (!Array.isArray(value) || value.length < schema.minItems || value.length > schema.maxItems) fail('invalid_input', `Invalid array at ${path}`);
    value.forEach((v, i) => validate(v, schema.items, `${path}[${i}]`));
  } else if (type === 'string') {
    if (typeof value !== 'string' || (schema.pattern && !new RegExp(schema.pattern).test(value))) fail('invalid_input', `Invalid string at ${path}`);
  } else if (type === 'number' || type === 'integer') {
    if (typeof value !== 'number' || !Number.isFinite(value) || (type === 'integer' && !Number.isSafeInteger(value)) || value < schema.minimum || value > schema.maximum) fail('invalid_input', `Invalid number at ${path}`);
  } else if (type === 'boolean' && typeof value !== 'boolean') fail('invalid_input', `Invalid boolean at ${path}`);
}
export function civil(date: string, time = '00:00:00', offset = 0) {
  const match = /^([+-]?[0-9]{4,6})-([0-9]{2})-([0-9]{2})$/.exec(date);
  const tm = /^([0-9]{2}):([0-9]{2}):([0-9]{2})$/.exec(time);
  if (!match || !tm) fail('invalid_date', 'Invalid civil date or time');
  const [year, month, day] = match.slice(1).map(Number);
  const [hour, minute, second] = tm.slice(1).map(Number);
  // Numeric fields avoid Date.UTC's special reinterpretation of years 0–99.
  const raw = new Date(0);
  raw.setUTCFullYear(year, month - 1, day);
  raw.setUTCHours(hour, minute, second, 0);
  const d = dayjs.utc(raw);
  if (!d.isValid() || d.year() !== year || d.month() + 1 !== month || d.date() !== day || d.hour() !== hour || d.minute() !== minute || d.second() !== second) fail('invalid_date', 'Invalid civil date or time');
  // Fixed-offset civil time, no host timezone/DST lookup or implicit Date parsing.
  // dayjs 1.11.19 utc.js converts even parsed HH:mm offsets with abs <= 16
  // as hours. Express those minute offsets as fractional hours explicitly.
  // Start from the UTC instant, then change display offset without keepLocalTime.
  const dayjsOffset = Math.abs(offset) <= 16 ? offset / 60 : offset;
  return dayjs.utc(raw.getTime() - offset * 60_000).utcOffset(dayjsOffset);
}
type Region = { code: string; name: string; children?: Region[] };
export function regionPath(codes: string[]) {
  let candidates = regions as Region[];
  const selected: Region[] = [];
  for (const code of codes) {
    const entry = candidates.find(v => v.code === code);
    if (!entry) fail('invalid_region', 'Region codes must be an existing parent-to-child path');
    selected.push(entry);
    candidates = entry.children ?? [];
  }
  return { selected, children: candidates };
}
export function location(codes: string[]) {
  const { selected } = regionPath(codes);
  for (const entry of [...selected].reverse()) {
    const code = entry.code.padEnd(6, '0');
    for (const candidate of [code, code.slice(0, 4) + '00', code.slice(0, 2) + '0000']) {
      const point = (coordinates as Record<string, { lng: number; lat: number }>)[candidate];
      if (point) return { codes, labels: selected.map(v => v.name), longitude: point.lng, latitude: point.lat, coordinate_code: candidate };
    }
  }
  return { codes, labels: selected.map(v => v.name), longitude: null, latitude: null, coordinate_code: null };
}
export function normalizeBirth(input: any) {
  const dt = civil(input.civil_date, input.civil_time, input.utc_offset_minutes);
  return { civil_date: input.civil_date, civil_time: input.civil_time, utc_offset_minutes: input.utc_offset_minutes, utc_datetime: dt.toISOString(), gender: input.gender, location: location(input.location_codes) };
}
