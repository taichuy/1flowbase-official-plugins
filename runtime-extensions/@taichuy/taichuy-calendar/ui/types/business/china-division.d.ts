/** Upstream china-division 2.7.0 province/city/area browser subset; codes are strings. */
export interface Province { code: string; name: string }
export interface City extends Province { provinceCode: string }
export interface Area extends City { cityCode: string }
/** A leaf has no children; each non-leaf's children have exactly the next level's shape. */
export interface DivisionBranch<Child> extends Province { children: Child[] }
export type DivisionTree<Levels extends readonly unknown[]> =
  Levels extends readonly [unknown, ...infer Rest]
    ? DivisionBranch<DivisionTree<Rest>> : Province;
export declare const provinces: Province[];
export declare const cities: City[];
export declare const areas: Area[];
export declare const pc: Record<string, string[]>;
export declare const pcC: DivisionTree<[unknown]>[];
export declare const pca: Record<string, Record<string, string[]>>;
export declare const pcaC: DivisionTree<[unknown, unknown]>[];
declare const chinaDivision: {
  provinces: typeof provinces;
  cities: typeof cities;
  areas: typeof areas;
  pc: typeof pc;
  pcC: typeof pcC;
  pca: typeof pca;
  pcaC: typeof pcaC;
};
export default chinaDivision;
