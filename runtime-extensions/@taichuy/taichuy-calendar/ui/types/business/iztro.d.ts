declare module "i18next" {
    // Internal Helpers
    import type { $Dictionary, $NormalizeIntoArray } from "i18next/typescript/helpers";
    import type { DefaultNamespace, FlatNamespace, FormatFunction, InitOptions, InterpolationOptions, Namespace, Resource, ResourceKey, ResourceLanguage, TOptions, } from "i18next/typescript/options";
    import type { KeyPrefix, TFunction } from "i18next/typescript/t";
    export interface WithT<Ns extends Namespace = DefaultNamespace> {
        // Expose parameterized t in the i18next interface hierarchy
        t: TFunction<Ns>;
    }
    export interface Interpolator {
        init(options: InterpolationOptions, reset: boolean): undefined;
        reset(): undefined;
        resetRegExp(): undefined;
        interpolate(str: string, data: object, lng: string, options: InterpolationOptions): string;
        nest(str: string, fc: (...args: any[]) => any, options: InterpolationOptions): string;
    }
    export class ResourceStore {
        constructor(data: Resource, options: InitOptions);
        public data: Resource;
        public options: InitOptions;
        /**
         * Gets fired when resources got added or removed
         */
        on(event: 'added' | 'removed', callback: (lng: string, ns: string) => void): void;
        /**
         * Remove event listener
         * removes all callback when callback not specified
         */
        off(event: 'added' | 'removed', callback?: (lng: string, ns: string) => void): void;
    }
    export interface Formatter {
        init(services: Services, i18nextOptions: InitOptions): void;
        add(name: string, fc: (value: any, lng: string | undefined, options: any) => string): void;
        addCached(name: string, fc: (lng: string | undefined, options: any) => (value: any) => string): void;
        format: FormatFunction;
    }
    export interface Services {
        backendConnector: any;
        i18nFormat: any;
        interpolator: Interpolator;
        languageDetector: any;
        languageUtils: any;
        logger: any;
        pluralResolver: any;
        resourceStore: ResourceStore;
        formatter?: Formatter;
    }
    export type ModuleType = 'backend' | 'logger' | 'languageDetector' | 'postProcessor' | 'i18nFormat' | 'formatter' | '3rdParty';
    export interface Module {
        type: ModuleType;
    }
    export type CallbackError = Error | string | null | undefined;
    export type ReadCallback = (err: CallbackError, data: ResourceKey | boolean | null | undefined) => void;
    export type MultiReadCallback = (err: CallbackError, data: Resource | null | undefined) => void;
    /**
     * Used to load data for i18next.
     * Can be provided as a singleton or as a prototype constructor (preferred for supporting multiple instances of i18next).
     * For singleton set property `type` to `'backend'` For a prototype constructor set static property.
     */
    export interface BackendModule<Options = object> extends Module {
        type: 'backend';
        init(services: Services, backendOptions: Options, i18nextOptions: InitOptions): void;
        read(language: string, namespace: string, callback: ReadCallback): void;
        /** Save the missing translation */
        create?(languages: readonly string[], namespace: string, key: string, fallbackValue: string): void;
        /** Load multiple languages and namespaces. For backends supporting multiple resources loading */
        readMulti?(languages: readonly string[], namespaces: readonly string[], callback: MultiReadCallback): void;
        /** Store the translation. For backends acting as cache layer */
        save?(language: string, namespace: string, data: ResourceLanguage): void;
    }
    /**
     * Used to detect language in user land.
     * Can be provided as a singleton or as a prototype constructor (preferred for supporting multiple instances of i18next).
     * For singleton set property `type` to `'languageDetector'` For a prototype constructor set static property.
     */
    export interface LanguageDetectorModule extends Module {
        type: 'languageDetector';
        init?(services: Services, detectorOptions: object, i18nextOptions: InitOptions): void;
        /** Must return detected language */
        detect(): string | readonly string[] | undefined;
        cacheUserLanguage?(lng: string): void;
    }
    /**
     * Used to detect language in user land.
     * Can be provided as a singleton or as a prototype constructor (preferred for supporting multiple instances of i18next).
     * For singleton set property `type` to `'languageDetector'` For a prototype constructor set static property.
     */
    export interface LanguageDetectorAsyncModule extends Module {
        type: 'languageDetector';
        /** Set to true to enable async detection */
        async: true;
        init?(services: Services, detectorOptions: object, i18nextOptions: InitOptions): void;
        /** Must call callback passing detected language or return a Promise */
        detect(callback: (lng: string | readonly string[] | undefined) => void | undefined): void | Promise<string | readonly string[] | undefined>;
        cacheUserLanguage?(lng: string): void | Promise<void>;
    }
    /**
     * Used to extend or manipulate the translated values before returning them in `t` function.
     * Need to be a singleton object.
     */
    export interface PostProcessorModule extends Module {
        /** Unique name */
        name: string;
        type: 'postProcessor';
        process(value: string, key: string | string[], options: TOptions, translator: any): string;
    }
    /**
     * Override the built-in console logger.
     * Do not need to be a prototype function.
     */
    export interface LoggerModule extends Module {
        type: 'logger';
        log(...args: any[]): void;
        warn(...args: any[]): void;
        error(...args: any[]): void;
    }
    export interface I18nFormatModule extends Module {
        type: 'i18nFormat';
    }
    export interface FormatterModule extends Module, Formatter {
        type: 'formatter';
    }
    export interface ThirdPartyModule extends Module {
        type: '3rdParty';
        init(i18next: i18n): void;
    }
    export interface Modules {
        backend?: BackendModule;
        logger?: LoggerModule;
        languageDetector?: LanguageDetectorModule | LanguageDetectorAsyncModule;
        i18nFormat?: I18nFormatModule;
        formatter?: FormatterModule;
        external: ThirdPartyModule[];
    }
    // helper to identify class https://stackoverflow.com/a/45983481/2363935
    export interface Newable<T> {
        new (...args: any[]): T;
    }
    export interface NewableModule<T extends Module> extends Newable<T> {
        type: T['type'];
    }
    export type Callback = (error: any, t: TFunction) => void;
    /**
     * Uses similar args as the t function and returns true if a key exists.
     */
    export interface ExistsFunction<TKeys extends string = string, TInterpolationMap extends object = $Dictionary> {
        (key: TKeys | TKeys[], options?: TOptions<TInterpolationMap>): boolean;
    }
    export interface CloneOptions extends InitOptions {
        /**
         * Will create a new instance of the resource store and import the existing translation resources.
         * This way it will not shared the resource store instance.
         * @default false
         */
        forkResourceStore?: boolean;
    }
    export interface CustomInstanceExtensions {
    }
    // Used just here to exclude `DefaultNamespace` which can be both string or array from `FlatNamespace`
    // in TFunction declaration below.
    // Due to this only very special usage I'm not moving this inside helpers.
    type InferArrayValuesElseReturnType<T> = T extends (infer A)[] ? A : T;
    // eslint-disable-next-line @typescript-eslint/naming-convention
    export interface i18n extends CustomInstanceExtensions {
        // Expose parameterized t in the i18next interface hierarchy
        t: TFunction<[
            ...$NormalizeIntoArray<DefaultNamespace>,
            ...Exclude<FlatNamespace, InferArrayValuesElseReturnType<DefaultNamespace>>[]
        ]>;
        /**
         * The default of the i18next module is an i18next instance ready to be initialized by calling init.
         * You can create additional instances using the createInstance function.
         *
         * @param options - Initial options.
         * @param callback - will be called after all translations were loaded or with an error when failed (in case of using a backend).
         */
        init(callback?: Callback): Promise<TFunction>;
        init<T>(options: InitOptions<T>, callback?: Callback): Promise<TFunction>;
        loadResources(callback?: (err: any) => void): void;
        /**
         * The use function is there to load additional plugins to i18next.
         * For available module see the plugins page and don't forget to read the documentation of the plugin.
         *
         * @param module Accepts a class or object
         */
        use<T extends Module>(module: T | NewableModule<T> | Newable<T>): this;
        /**
         * List of modules used
         */
        modules: Modules;
        /**
         * Internal container for all used plugins and implementation details like languageUtils, pluralResolvers, etc.
         */
        services: Services;
        /**
         * Internal container for translation resources
         */
        store: ResourceStore;
        /**
         * Uses similar args as the t function and returns true if a key exists.
         */
        exists: ExistsFunction;
        /**
         * Returns a resource data by language.
         */
        getDataByLanguage(lng: string): {
            [key: string]: {
                [key: string]: string;
            };
        } | undefined;
        /**
         * Returns a t function that defaults to given language or namespace.
         * Both params could be arrays of languages or namespaces and will be treated as fallbacks in that case.
         * On the returned function you can like in the t function override the languages or namespaces by passing them in options or by prepending namespace.
         *
         * Accepts optional keyPrefix that will be automatically applied to returned t function.
         */
        getFixedT<Ns extends Namespace | null = DefaultNamespace, TKPrefix extends KeyPrefix<ActualNs> = undefined, ActualNs extends Namespace = Ns extends null ? DefaultNamespace : Ns>(...args: [
            lng: string | readonly string[],
            ns?: Ns,
            keyPrefix?: TKPrefix
        ] | [
            lng: null,
            ns: Ns,
            keyPrefix?: TKPrefix
        ]): TFunction<ActualNs, TKPrefix>;
        /**
         * Changes the language. The callback will be called as soon translations were loaded or an error occurs while loading.
         * HINT: For easy testing - setting lng to 'cimode' will set t function to always return the key.
         */
        changeLanguage(lng?: string, callback?: Callback): Promise<TFunction>;
        /**
         * Is set to the current detected or set language.
         * If you need the primary used language depending on your configuration (supportedLngs, load) you will prefer using i18next.languages[0].
         */
        language: string;
        /**
         * Is set to an array of language-codes that will be used it order to lookup the translation value.
         */
        languages: readonly string[];
        /**
         * Is set to the current resolved language.
         * It can be used as primary used language, for example in a language switcher.
         */
        resolvedLanguage?: string;
        /**
         * Checks if namespace has loaded yet.
         * i.e. used by react-i18next
         */
        hasLoadedNamespace(ns: string | readonly string[], options?: {
            lng?: string | readonly string[];
            fallbackLng?: InitOptions['fallbackLng'];
            /**
             * if `undefined` is returned default checks are performed.
             */
            precheck?: (i18n: i18n,
            /**
             * Check if the language namespace provided are not in loading status:
             * returns `true` if load is completed successfully or with an error.
             */
            loadNotPending: (lng: string | readonly string[], ns: string | readonly string[]) => boolean) => boolean | undefined;
        }): boolean;
        /**
         * Loads additional namespaces not defined in init options.
         */
        loadNamespaces(ns: string | readonly string[], callback?: Callback): Promise<void>;
        /**
         * Loads additional languages not defined in init options (preload).
         */
        loadLanguages(lngs: string | readonly string[], callback?: Callback): Promise<void>;
        /**
         * Reloads resources on given state. Optionally you can pass an array of languages and namespaces as params if you don't want to reload all.
         */
        reloadResources(lngs?: string | readonly string[], ns?: string | readonly string[], callback?: () => void): Promise<void>;
        reloadResources(lngs: null, ns: string | readonly string[], callback?: () => void): Promise<void>;
        /**
         * Changes the default namespace.
         */
        setDefaultNamespace(ns: string): void;
        /**
         * Returns rtl or ltr depending on languages read direction.
         */
        dir(lng?: string): 'ltr' | 'rtl';
        /**
         * Exposes interpolation.format function added on init.
         */
        format: FormatFunction;
        /**
         * Will return a new i18next instance.
         * Please read the options page for details on configuration options.
         * Providing a callback will automatically call init.
         * The callback will be called after all translations were loaded or with an error when failed (in case of using a backend).
         */
        createInstance(options?: InitOptions, callback?: Callback): i18n;
        /**
         * Creates a clone of the current instance. Shares store, plugins and initial configuration.
         * Can be used to create an instance sharing storage but being independent on set language or namespaces.
         */
        cloneInstance(options?: CloneOptions, callback?: Callback): i18n;
        /**
         * Gets fired after initialization.
         */
        on(event: 'initialized', callback: (options: InitOptions) => void): void;
        /**
         * Gets fired on loaded resources.
         */
        on(event: 'loaded', callback: (loaded: {
            [language: string]: {
                [namespace: string]: boolean;
            };
        }) => void): void;
        /**
         * Gets fired if loading resources failed.
         */
        on(event: 'failedLoading', callback: (lng: string, ns: string, msg: string) => void): void;
        /**
         * Gets fired on accessing a key not existing.
         */
        on(event: 'missingKey', callback: (lngs: readonly string[], namespace: string, key: string, res: string) => void): void;
        /**
         * Gets fired when resources got added or removed.
         */
        on(event: 'added' | 'removed', callback: (lng: string, ns: string) => void): void;
        /**
         * Gets fired when changeLanguage got called.
         */
        on(event: 'languageChanged', callback: (lng: string) => void): void;
        /**
         * Event listener
         */
        on(event: string, listener: (...args: any[]) => void): void;
        /**
         * Remove event listener
         * removes all callback when callback not specified
         */
        off(event: string, listener?: (...args: any[]) => void): void;
        /**
         * Gets one value by given key.
         */
        getResource(lng: string, ns: string, key: string, options?: Pick<InitOptions, 'keySeparator' | 'ignoreJSONStructure'>): any;
        /**
         * Adds one key/value.
         */
        addResource(lng: string, ns: string, key: string, value: string, options?: {
            keySeparator?: string;
            silent?: boolean;
        }): i18n;
        /**
         * Adds multiple key/values.
         */
        addResources(lng: string, ns: string, resources: any): i18n;
        /**
         * Adds a complete bundle.
         * Setting deep param to true will extend existing translations in that file.
         * Setting overwrite to true it will overwrite existing translations in that file.
         */
        addResourceBundle(lng: string, ns: string, resources: any, deep?: boolean, overwrite?: boolean): i18n;
        /**
         * Checks if a resource bundle exists.
         */
        hasResourceBundle(lng: string, ns: string): boolean;
        /**
         * Returns a resource bundle.
         */
        getResourceBundle(lng: string, ns: string): any;
        /**
         * Removes an existing bundle.
         */
        removeResourceBundle(lng: string, ns: string): i18n;
        /**
         * Current options
         */
        options: InitOptions;
        /**
         * Is initialized
         */
        isInitialized: boolean;
        /**
         * Is initializing
         */
        isInitializing: boolean;
        /**
         * Store was initialized
         */
        initializedStoreOnce: boolean;
        /**
         * Language was initialized
         */
        initializedLanguageOnce: boolean;
        /**
         * Emit event
         */
        emit(eventName: string, ...args: any[]): void;
    }
    export type * from "i18next/typescript/options";
    export type {
    // we need to explicitely export some types, to prevent some issues with next-i18next and interpolation variable validation, etc...
    FallbackLngObjList, FallbackLng, InitOptions, TypeOptions, CustomTypeOptions, CustomPluginOptions, PluginOptions, FormatFunction, InterpolationOptions, ReactOptions, ResourceKey, ResourceLanguage, Resource, TOptions, Namespace, DefaultNamespace, FlatNamespace, } from "i18next/typescript/options";
    export type * from "i18next/typescript/t";
    export type { TFunction, ParseKeys, TFunctionReturn, TFunctionDetailedResult, KeyPrefix, } from "i18next/typescript/t";
    const i18next: i18n;
    export default i18next;
    export const createInstance: i18n['createInstance'];
    export const dir: i18n['dir'];
    export const init: i18n['init'];
    export const loadResources: i18n['loadResources'];
    export const reloadResources: i18n['reloadResources'];
    export const use: i18n['use'];
    export const changeLanguage: i18n['changeLanguage'];
    export const getFixedT: i18n['getFixedT'];
    export const t: i18n['t'];
    export const exists: i18n['exists'];
    export const setDefaultNamespace: i18n['setDefaultNamespace'];
    export const hasLoadedNamespace: i18n['hasLoadedNamespace'];
    export const loadNamespaces: i18n['loadNamespaces'];
    export const loadLanguages: i18n['loadLanguages'];
}

declare module "i18next/typescript/helpers" {
    // Types
    export type $Dictionary<T = unknown> = {
        [key: string]: T;
    };
    export type $SpecialObject = object | Array<string | object>;
    // Types Operators
    export type $MergeBy<T, K> = Omit<T, keyof K> & K;
    export type $OmitArrayKeys<Arr> = Arr extends readonly any[] ? Omit<Arr, keyof any[]> : Arr;
    export type $PreservedValue<Value, Fallback> = [
        Value
    ] extends [
        never
    ] ? Fallback : Value;
    export type $NormalizeIntoArray<T extends unknown | readonly unknown[]> = T extends readonly unknown[] ? T : [
        T
    ];
    /**
     * @typeParam T
     * @example
     * ```
     * $UnionToIntersection<{foo: {bar: string} | {asd: boolean}}> = {foo: {bar: string} & {asd: boolean}}
     * ```
     *
     * @see https://stackoverflow.com/questions/50374908/transform-union-type-to-intersection-type
     */
    type $UnionToIntersection<T> = (T extends unknown ? (k: T) => void : never) extends (k: infer I) => void ? I : never;
    /**
     * @typeParam TPath union of strings
     * @typeParam TValue value of the record
     * @example
     * ```
     * $StringKeyPathToRecord<'foo.bar' | 'asd'> = {foo: {bar: string} | {asd: boolean}}
     * ```
     */
    type $StringKeyPathToRecordUnion<TPath extends string, TValue> = TPath extends `${infer TKey}.${infer Rest}` ? {
        [Key in TKey]: $StringKeyPathToRecord<Rest, TValue>;
    } : {
        [Key in TPath]: TValue;
    };
    /**
     * Used to intersect output of {@link $StringKeyPathToRecordUnion}
     *
     * @typeParam TPath union of strings
     * @typeParam TValue value of the record
     * @example
     * ```
     * $StringKeyPathToRecord<'foo.bar' | 'asd'> = {foo: {bar: string} & {asd: boolean}}
     * ```
     */
    export type $StringKeyPathToRecord<TPath extends string, TValue> = $UnionToIntersection<$StringKeyPathToRecordUnion<TPath, TValue>>;
    /**
     * We could use NoInfer typescript build-in utility,
     * however this project still supports ts < 5.4.
     *
     * @see https://github.com/millsp/ts-toolbelt/blob/master/sources/Function/NoInfer.ts
     */
    export type $NoInfer<A> = [
        A
    ][A extends any ? 0 : never];
}

declare module "i18next/typescript/options" {
    import type { $MergeBy, $PreservedValue, $Dictionary } from "i18next/typescript/helpers";
    /**
     * This interface can be augmented by users to add types to `i18next` default TypeOptions.
     *
     * Usage:
     * ```ts
     * // i18next.d.ts
     * import 'i18next';
     * declare module 'i18next' {
     *   interface CustomTypeOptions {
     *     defaultNS: 'custom';
     *     returnNull: false;
     *     returnObjects: false;
     *     nsSeparator: ':';
     *     keySeparator: '.';
     *     jsonFormat: 'v4';
     *     allowObjectInHTMLChildren: false;
     *     resources: {
     *       custom: {
     *         foo: 'foo';
     *       };
     *     };
     *   }
     * }
     * ```
     */
    export interface CustomTypeOptions {
    }
    /**
     * This interface can be augmented by users to add types to `i18next` default PluginOptions.
     */
    export interface CustomPluginOptions {
    }
    export type TypeOptions = $MergeBy<{
        /**
         * Allows null values as valid translation
         */
        returnNull: false;
        /**
         * Allows empty string as valid translation
         */
        returnEmptyString: true;
        /**
         * Allows objects as valid translation result
         */
        returnObjects: false;
        /**
         * Char to separate keys
         */
        keySeparator: '.';
        /**
         * Char to split namespace from key
         */
        nsSeparator: ':';
        /**
         * Char to split plural from key
         */
        pluralSeparator: '_';
        /**
         * Char to split context from key
         */
        contextSeparator: '_';
        /**
         * Default namespace used if not passed to translation function
         */
        defaultNS: 'translation';
        /**
         * Fallback namespace used if translation not found in given namespace
         * @default false
         */
        fallbackNS: false;
        /**
         * Json Format Version - V4 allows plural suffixes
         */
        jsonFormat: 'v4';
        /**
         * Resources to initialize with
         */
        resources: object;
        /**
         * Flag that allows HTML elements to receive objects. This is only useful for React applications
         * where you pass objects to HTML elements so they can be replaced to their respective interpolation
         * values (mostly with Trans component)
         */
        allowObjectInHTMLChildren: false;
        /**
         * Prefix for interpolation
         */
        interpolationPrefix: '{{';
        /**
         * Suffix for interpolation
         */
        interpolationSuffix: '}}';
        /**
         * Prefix for unescaped interpolation
         */
        unescapePrefix: '-';
        /**
         * Suffix for unescaped interpolation
         */
        unescapeSuffix: '';
    }, CustomTypeOptions>;
    export type PluginOptions<T> = $MergeBy<{
        /**
         * Options for language detection - check documentation of plugin
         * @default undefined
         */
        detection?: object;
        /**
         * Options for backend - check documentation of plugin
         * @default undefined
         */
        backend?: T;
        /**
         * Options for cache layer - check documentation of plugin
         * @default undefined
         */
        cache?: object;
        /**
         * Options for i18n message format - check documentation of plugin
         * @default undefined
         */
        i18nFormat?: object;
    }, CustomPluginOptions>;
    export type FormatFunction = (value: any, format?: string, lng?: string, options?: InterpolationOptions & $Dictionary<any>) => string;
    export interface InterpolationOptions {
        /**
         * Format function see formatting for details
         * @default noop
         */
        format?: FormatFunction;
        /**
         * Used to separate format from interpolation value
         * @default ','
         */
        formatSeparator?: string;
        /**
         * Escape function
         * @default str => str
         */
        escape?(str: string): string;
        /**
         * Always format interpolated values.
         * @default false
         */
        alwaysFormat?: boolean;
        /**
         * Escape passed in values to avoid xss injection
         * @default true
         */
        escapeValue?: boolean;
        /**
         * If true, then value passed into escape function is not casted to string, use with custom escape function that does its own type check
         * @default false
         */
        useRawValueToEscape?: boolean;
        /**
         * Prefix for interpolation
         * @default '{{'
         */
        prefix?: string;
        /**
         * Suffix for interpolation
         * @default '}}'
         */
        suffix?: string;
        /**
         * Escaped prefix for interpolation (regexSafe)
         * @default undefined
         */
        prefixEscaped?: string;
        /**
         * Escaped suffix for interpolation (regexSafe)
         * @default undefined
         */
        suffixEscaped?: string;
        /**
         * Suffix to unescaped mode
         * @default undefined
         */
        unescapeSuffix?: string;
        /**
         * Prefix to unescaped mode
         * @default '-'
         */
        unescapePrefix?: string;
        /**
         * Prefix for nesting
         * @default '$t('
         */
        nestingPrefix?: string;
        /**
         * Suffix for nesting
         * @default ')'
         */
        nestingSuffix?: string;
        /**
         * Escaped prefix for nesting (regexSafe)
         * @default undefined
         */
        nestingPrefixEscaped?: string;
        /**
         * Escaped suffix for nesting (regexSafe)
         * @default undefined
         */
        nestingSuffixEscaped?: string;
        /**
         * Separates options from key
         * @default ','
         */
        nestingOptionsSeparator?: string;
        /**
         * Global variables to use in interpolation replacements
         * @default undefined
         */
        defaultVariables?: {
            [index: string]: any;
        };
        /**
         * After how many interpolation runs to break out before throwing a stack overflow
         * @default 1000
         */
        maxReplaces?: number;
        /**
         * If true, it will skip to interpolate the variables
         * @default true
         */
        skipOnVariables?: boolean;
    }
    export interface FallbackLngObjList {
        [language: string]: readonly string[];
    }
    export type FallbackLng = string | readonly string[] | FallbackLngObjList | ((code: string) => string | readonly string[] | FallbackLngObjList);
    export interface ReactOptions {
        /**
         * Set it to fallback to let passed namespaces to translated hoc act as fallbacks
         * @default 'default'
         */
        nsMode?: 'default' | 'fallback';
        /**
         * Set it to the default parent element created by the Trans component.
         * @default 'div'
         */
        defaultTransParent?: string;
        /**
         * Set which events trigger a re-render, can be set to false or string of events
         * @default 'languageChanged'
         */
        bindI18n?: string | false;
        /**
         * Set which events on store trigger a re-render, can be set to false or string of events
         * @default ''
         */
        bindI18nStore?: string | false;
        /**
         * Set fallback value for Trans components without children
         * @default undefined
         */
        transEmptyNodeValue?: string;
        /**
         * Set it to false if you do not want to use Suspense
         * @default true
         */
        useSuspense?: boolean;
        /**
         * Function to generate an i18nKey from the defaultValue (or Trans children)
         * when no key is provided.
         * By default, the defaultValue (Trans text) itself is used as the key.
         * If you want to require keys for all translations, supply a function
         * that always throws an error.
         * @default undefined
         */
        hashTransKey?(defaultValue: TOptionsBase['defaultValue']): TOptionsBase['defaultValue'];
        /**
         * Convert eg. <br/> found in translations to a react component of type br
         * @default true
         */
        transSupportBasicHtmlNodes?: boolean;
        /**
         * Which nodes not to convert in defaultValue generation in the Trans component.
         * @default ['br', 'strong', 'i', 'p']
         */
        transKeepBasicHtmlNodesFor?: readonly string[];
        /**
         * Wrap text nodes in a user-specified element.
         * @default ''
         */
        transWrapTextNodes?: string;
        /**
         * Optional keyPrefix that will be automatically applied to returned t function in useTranslation for example.
         * @default undefined
         */
        keyPrefix?: string;
        /**
         * Unescape function
         * by default it unescapes some basic html entities
         */
        unescape?(str: string): string;
    }
    export type ResourceKey = string | {
        [key: string]: any;
    };
    export interface ResourceLanguage {
        [namespace: string]: ResourceKey;
    }
    export interface Resource {
        [language: string]: ResourceLanguage;
    }
    export interface InitOptions<T = object> extends PluginOptions<T> {
        /**
         * Logs info level to console output. Helps finding issues with loading not working.
         * @default false
         */
        debug?: boolean;
        /**
         * Resources to initialize with (if not using loading or not appending using addResourceBundle)
         * @default undefined
         */
        resources?: Resource;
        /**
         * Allow initializing with bundled resources while using a backend to load non bundled ones.
         * @default false
         */
        partialBundledLanguages?: boolean;
        /**
         * Language to use (overrides language detection)
         * @default undefined
         */
        lng?: string;
        /**
         * Language to use if translations in user language are not available.
         * @default 'dev'
         */
        fallbackLng?: false | FallbackLng;
        /**
         * Array of allowed languages
         * @default false
         */
        supportedLngs?: false | readonly string[];
        /**
         * If true will pass eg. en-US if finding en in supportedLngs
         * @default false
         */
        nonExplicitSupportedLngs?: boolean;
        /**
         * Language codes to lookup, given set language is
         * 'en-US': 'all' --> ['en-US', 'en', 'dev'],
         * 'currentOnly' --> 'en-US',
         * 'languageOnly' --> 'en'
         * @default 'all'
         */
        load?: 'all' | 'currentOnly' | 'languageOnly';
        /**
         * Array of languages to preload. Important on server-side to assert translations are loaded before rendering views.
         * @default false
         */
        preload?: false | readonly string[];
        /**
         * Language will be lowercased eg. en-US --> en-us
         * @default false
         */
        lowerCaseLng?: boolean;
        /**
         * Language will be lowercased EN --> en while leaving full locales like en-US
         * @default false
         */
        cleanCode?: boolean;
        /**
         * String or array of namespaces to load
         * @default 'translation'
         */
        ns?: string | readonly string[];
        /**
         * Default namespace used if not passed to translation function
         * @default 'translation'
         */
        defaultNS?: string | false | readonly string[];
        /**
         * String or array of namespaces to lookup key if not found in given namespace.
         * @default false
         */
        fallbackNS?: false | string | readonly string[];
        /**
         * Calls save missing key function on backend if key not found.
         * @default false
         */
        saveMissing?: boolean;
        /**
         * Calls save missing key function on backend if key not found also for plural forms.
         * @default false
         */
        saveMissingPlurals?: boolean;
        /**
         * Experimental: enable to update default values using the saveMissing
         * (Works only if defaultValue different from translated value.
         * Only useful on initial development or when keeping code as source of truth not changing values outside of code.
         * Only supported if backend supports it already)
         * @default false
         */
        updateMissing?: boolean;
        /**
         * @default 'fallback'
         */
        saveMissingTo?: 'current' | 'all' | 'fallback';
        /**
         * Used to not fallback to the key as default value, when using saveMissing functionality.
         * i.e. when using with i18next-http-backend this will result in having a key with an empty string value.
         * @default false
         */
        missingKeyNoValueFallbackToKey?: boolean;
        /**
         * Used for custom missing key handling (needs saveMissing set to true!)
         * @default false
         */
        missingKeyHandler?: false | ((lngs: readonly string[], ns: string, key: string, fallbackValue: string, updateMissing: boolean, options: any) => void);
        /**
         * Receives a key that was not found in `t()` and returns a value, that will be returned by `t()`
         * @default noop
         */
        parseMissingKeyHandler?(key: string, defaultValue?: string): any;
        /**
         * Appends namespace to missing key
         * @default false
         */
        appendNamespaceToMissingKey?: boolean;
        /**
         * Gets called in case a interpolation value is undefined. This method will not be called if the value is empty string or null
         * @default noop
         */
        missingInterpolationHandler?: (text: string, value: any, options: InitOptions) => any;
        /**
         * Will use 'plural' as suffix for languages only having 1 plural form, setting it to false will suffix all with numbers
         * @default true
         */
        simplifyPluralSuffix?: boolean;
        /**
         * String or array of postProcessors to apply per default
         * @default false
         */
        postProcess?: false | string | readonly string[];
        /**
         * passthrough the resolved object including 'usedNS', 'usedLang' etc into options object of postprocessors as 'i18nResolved' property
         * @default false
         */
        postProcessPassResolved?: boolean;
        /**
         * Allows null values as valid translation
         * @default false
         */
        returnNull?: boolean;
        /**
         * Allows empty string as valid translation
         * @default true
         */
        returnEmptyString?: boolean;
        /**
         * Allows objects as valid translation result
         * @default false
         */
        returnObjects?: boolean;
        /**
         * Returns an object that includes information about the used language, namespace, key and value
         * @default false
         */
        returnDetails?: boolean;
        /**
         * Gets called if object was passed in as key but returnObjects was set to false
         * @default noop
         */
        returnedObjectHandler?(key: string, value: string, options: any): void;
        /**
         * Char, eg. '\n' that arrays will be joined by
         * @default false
         */
        joinArrays?: false | string;
        /**
         * Sets defaultValue
         * @default args => ({ defaultValue: args[1] })
         */
        overloadTranslationOptionHandler?(args: string[]): TOptions;
        /**
         * @see https://www.i18next.com/translation-function/interpolation
         */
        interpolation?: InterpolationOptions;
        /**
         * Options for react - check documentation of plugin
         * @default undefined
         */
        react?: ReactOptions;
        /**
         * Triggers resource loading in init function inside a setTimeout (default async behaviour).
         * Set it to false if your backend loads resources sync - that way calling i18next.t after
         * init is possible without relaying on the init callback.
         * @default true
         */
        initImmediate?: boolean;
        /**
         * Char to separate keys
         * @default '.'
         */
        keySeparator?: false | string;
        /**
         * Char to split namespace from key
         * @default ':'
         */
        nsSeparator?: false | string;
        /**
         * Char to split plural from key
         * @default '_'
         */
        pluralSeparator?: string;
        /**
         * Char to split context from key
         * @default '_'
         */
        contextSeparator?: string;
        /**
         * Prefixes the namespace to the returned key when using `cimode`
         * @default false
         */
        appendNamespaceToCIMode?: boolean;
        /**
         * Compatibility JSON version
         * @default 'v4'
         */
        compatibilityJSON?: 'v1' | 'v2' | 'v3' | 'v4';
        /**
         * Options for https://github.com/locize/locize-lastused
         * @default undefined
         */
        locizeLastUsed?: {
            /**
             * The id of your locize project
             */
            projectId: string;
            /**
             * An api key if you want to send missing keys
             */
            apiKey?: string;
            /**
             * The reference language of your project
             * @default 'en'
             */
            referenceLng?: string;
            /**
             * Version
             * @default 'latest'
             */
            version?: string;
            /**
             * Debounce interval to send data in milliseconds
             * @default 90000
             */
            debounceSubmit?: number;
            /**
             * Hostnames that are allowed to send last used data.
             * Please keep those to your local system, staging, test servers (not production)
             * @default ['localhost']
             */
            allowedHosts?: readonly string[];
        };
        /**
         * Automatically lookup for a flat key if a nested key is not found an vice-versa
         * @default true
         */
        ignoreJSONStructure?: boolean;
        /**
         * Limit parallelism of calls to backend
         * This is needed to prevent trying to open thousands of
         * sockets or file descriptors, which can cause failures
         * and actually make the entire process take longer.
         * @default 10
         */
        maxParallelReads?: number;
        /**
         * The maximum number of retries to perform.
         * Note that retries are only performed when a request has no response
         * and throws an error.
         * The default value is used if value is set below 0.
         * @default 5
         */
        maxRetries?: number;
        /**
         * Set how long to wait, in milliseconds, between retries of failed requests.
         * This number is compounded by a factor of 2 for subsequent retry.
         * The default value is used if value is set below 1ms.
         * @default 350
         */
        retryTimeout?: number;
    }
    export interface TOptionsBase {
        /**
         * Default value to return if a translation was not found
         */
        defaultValue?: unknown;
        /**
         * Count value used for plurals
         */
        count?: number;
        /**
         * Ordinal flag for ordinal plurals
         */
        ordinal?: boolean;
        /**
         * Used for contexts (eg. male\female)
         */
        context?: unknown;
        /**
         * Object with vars for interpolation - or put them directly in options
         */
        replace?: any;
        /**
         * Override language to use
         */
        lng?: string;
        /**
         * Override languages to use
         */
        lngs?: readonly string[];
        /**
         * Override language to lookup key if not found see fallbacks for details
         */
        fallbackLng?: FallbackLng;
        /**
         * Override namespaces (string or array)
         */
        ns?: Namespace;
        /**
         * Override char to separate keys
         */
        keySeparator?: false | string;
        /**
         * Override char to split namespace from key
         */
        nsSeparator?: false | string;
        /**
         * Accessing an object not a translation string (can be set globally too)
         */
        returnObjects?: boolean;
        /**
         * Returns an object that includes information about the used language, namespace, key and value
         */
        returnDetails?: boolean;
        /**
         * Char, eg. '\n' that arrays will be joined by (can be set globally too)
         */
        joinArrays?: string;
        /**
         * String or array of postProcessors to apply see interval plurals as a sample
         */
        postProcess?: string | readonly string[];
        /**
         * Override interpolation options
         */
        interpolation?: InterpolationOptions;
    }
    export type TOptions<TInterpolationMap extends object = $Dictionary> = TOptionsBase & TInterpolationMap;
    export type FlatNamespace = $PreservedValue<keyof TypeOptions['resources'], string>;
    export type Namespace<T = FlatNamespace> = T | readonly T[];
    export type DefaultNamespace = TypeOptions['defaultNS'];
}

declare module "i18next/typescript/t" {
    import type { $OmitArrayKeys, $PreservedValue, $Dictionary, $SpecialObject, $StringKeyPathToRecord, $NoInfer, } from "i18next/typescript/helpers";
    import type { TypeOptions, Namespace, FlatNamespace, DefaultNamespace, TOptions, } from "i18next/typescript/options";
    /** @todo consider to replace {} with Record<string, never> */
    /* eslint @typescript-eslint/ban-types: ['error', { types: { "{}": false } }] */
    // Type Options
    type _ReturnObjects = TypeOptions['returnObjects'];
    type _ReturnEmptyString = TypeOptions['returnEmptyString'];
    type _ReturnNull = TypeOptions['returnNull'];
    type _KeySeparator = TypeOptions['keySeparator'];
    type _NsSeparator = TypeOptions['nsSeparator'];
    type _PluralSeparator = TypeOptions['pluralSeparator'];
    type _ContextSeparator = TypeOptions['contextSeparator'];
    type _FallbackNamespace = TypeOptions['fallbackNS'];
    type _Resources = TypeOptions['resources'];
    type _JSONFormat = TypeOptions['jsonFormat'];
    type _InterpolationPrefix = TypeOptions['interpolationPrefix'];
    type _InterpolationSuffix = TypeOptions['interpolationSuffix'];
    type _UnescapePrefix = TypeOptions['unescapePrefix'];
    type _UnescapeSuffix = TypeOptions['unescapeSuffix'];
    type $IsResourcesDefined = [
        keyof _Resources
    ] extends [
        never
    ] ? false : true;
    type $ValueIfResourcesDefined<Value, Fallback> = $IsResourcesDefined extends true ? Value : Fallback;
    type $FirstNamespace<Ns extends Namespace> = Ns extends readonly any[] ? Ns[0] : Ns;
    type Resources = $ValueIfResourcesDefined<_Resources, $Dictionary<string>>;
    type PluralSuffix = _JSONFormat extends 'v4' ? 'zero' | 'one' | 'two' | 'few' | 'many' | 'other' : number | 'plural';
    type WithOrWithoutPlural<Key> = _JSONFormat extends 'v4' | 'v3' ? Key extends `${infer KeyWithoutOrdinalPlural}${_PluralSeparator}ordinal${_PluralSeparator}${PluralSuffix}` ? KeyWithoutOrdinalPlural | Key : Key extends `${infer KeyWithoutPlural}${_PluralSeparator}${PluralSuffix}` ? KeyWithoutPlural | Key : Key : Key;
    type JoinKeys<K1, K2> = `${K1 & string}${_KeySeparator}${K2 & string}`;
    type AppendNamespace<Ns, Keys> = `${Ns & string}${_NsSeparator}${Keys & string}`;
    type TrimSpaces<T extends string, Acc extends string = ''> = T extends `${infer Char}${infer Rest}` ? Char extends ' ' ? TrimSpaces<Rest, Acc> : TrimSpaces<Rest, `${Acc}${Char}`> : T extends '' ? Acc : never;
    /** ****************************************************
     * Build all keys and key prefixes based on Resources *
     ***************************************************** */
    type KeysBuilderWithReturnObjects<Res, Key = keyof Res> = Key extends keyof Res ? Res[Key] extends $Dictionary | readonly unknown[] ? JoinKeys<Key, WithOrWithoutPlural<keyof $OmitArrayKeys<Res[Key]>>> | JoinKeys<Key, KeysBuilderWithReturnObjects<Res[Key]>> : never : never;
    type KeysBuilderWithoutReturnObjects<Res, Key = keyof $OmitArrayKeys<Res>> = Key extends keyof Res ? Res[Key] extends $Dictionary | readonly unknown[] ? JoinKeys<Key, KeysBuilderWithoutReturnObjects<Res[Key]>> : Key : never;
    type KeysBuilder<Res, WithReturnObjects> = $IsResourcesDefined extends true ? WithReturnObjects extends true ? keyof Res | KeysBuilderWithReturnObjects<Res> : KeysBuilderWithoutReturnObjects<Res> : string;
    type KeysWithReturnObjects = {
        [Ns in FlatNamespace]: WithOrWithoutPlural<KeysBuilder<Resources[Ns], true>>;
    };
    type KeysWithoutReturnObjects = {
        [Ns in FlatNamespace]: WithOrWithoutPlural<KeysBuilder<Resources[Ns], false>>;
    };
    type ResourceKeys<WithReturnObjects = _ReturnObjects> = WithReturnObjects extends true ? KeysWithReturnObjects : KeysWithoutReturnObjects;
    /** **********************************************************************
     * Parse t function keys based on the namespace, options and key prefix *
     *********************************************************************** */
    export type KeysByTOptions<TOpt extends TOptions> = TOpt['returnObjects'] extends true ? ResourceKeys<true> : ResourceKeys;
    export type NsByTOptions<Ns extends Namespace, TOpt extends TOptions> = TOpt['ns'] extends Namespace ? TOpt['ns'] : Ns;
    type ParseKeysByKeyPrefix<Keys, KPrefix> = KPrefix extends string ? Keys extends `${KPrefix}${_KeySeparator}${infer Key}` ? Key : never : Keys;
    type ParseKeysByNamespaces<Ns extends Namespace, Keys> = Ns extends readonly (infer UnionNsps)[] ? UnionNsps extends keyof Keys ? AppendNamespace<UnionNsps, Keys[UnionNsps]> : never : never;
    type ParseKeysByFallbackNs<Keys extends $Dictionary> = _FallbackNamespace extends false ? never : _FallbackNamespace extends (infer UnionFallbackNs extends string)[] ? Keys[UnionFallbackNs] : Keys[_FallbackNamespace & string];
    export type FilterKeysByContext<Keys, Context> = Context extends string ? Keys extends `${infer Prefix}${_ContextSeparator}${Context}${_PluralSeparator}${PluralSuffix}` | `${infer Prefix}${_ContextSeparator}${Context}` ? Prefix : never : Keys;
    export type ParseKeys<Ns extends Namespace = DefaultNamespace, TOpt extends TOptions = {}, KPrefix = undefined, Keys extends $Dictionary = KeysByTOptions<TOpt>, ActualNS extends Namespace = NsByTOptions<Ns, TOpt>, Context extends TOpt['context'] = TOpt['context']> = $IsResourcesDefined extends true ? FilterKeysByContext<ParseKeysByKeyPrefix<Keys[$FirstNamespace<ActualNS>], KPrefix> | ParseKeysByNamespaces<ActualNS, Keys> | ParseKeysByFallbackNs<Keys>, Context> : string;
    /** *******************************************************
     * Parse t function return type and interpolation values *
     ******************************************************** */
    type ParseActualValue<Ret> = Ret extends `${_UnescapePrefix}${infer ActualValue}${_UnescapeSuffix}` ? TrimSpaces<ActualValue> : Ret;
    type ParseInterpolationValues<Ret> = Ret extends `${string}${_InterpolationPrefix}${infer Value}${_InterpolationSuffix}${infer Rest}` ? (Value extends `${infer ActualValue},${string}` ? ParseActualValue<ActualValue> : ParseActualValue<Value>) | ParseInterpolationValues<Rest> : never;
    type InterpolationMap<Ret> = $PreservedValue<$StringKeyPathToRecord<ParseInterpolationValues<Ret>, unknown>, Record<string, unknown>>;
    type ParseTReturnPlural<Res, Key, KeyWithPlural = `${Key & string}${_PluralSeparator}${PluralSuffix}`> = Res[(KeyWithPlural | Key) & keyof Res];
    type ParseTReturnPluralOrdinal<Res, Key, KeyWithOrdinalPlural = `${Key & string}${_PluralSeparator}ordinal${_PluralSeparator}${PluralSuffix}`> = Res[(KeyWithOrdinalPlural | Key) & keyof Res];
    type ParseTReturnWithFallback<Key, Val> = Val extends '' ? _ReturnEmptyString extends true ? '' : Key : Val extends null ? _ReturnNull extends true ? null : Key : Val;
    type ParseTReturn<Key, Res, TOpt extends TOptions = {}> = ParseTReturnWithFallback<Key, Key extends `${infer K1}${_KeySeparator}${infer RestKey}` ? ParseTReturn<RestKey, Res[K1 & keyof Res], TOpt> : TOpt['count'] extends number ? TOpt['ordinal'] extends boolean ? ParseTReturnPluralOrdinal<Res, Key> : ParseTReturnPlural<Res, Key> : Res extends readonly unknown[] ? Key extends `${infer NKey extends number}` ? Res[NKey] : never : Res[Key & keyof Res]>;
    type TReturnOptionalNull = _ReturnNull extends true ? null : never;
    type TReturnOptionalObjects<TOpt extends TOptions> = _ReturnObjects extends true ? $SpecialObject | string : TOpt['returnObjects'] extends true ? $SpecialObject : string;
    type DefaultTReturn<TOpt extends TOptions> = TReturnOptionalObjects<TOpt> | TReturnOptionalNull;
    export type KeyWithContext<Key, TOpt extends TOptions> = TOpt['context'] extends string ? `${Key & string}${_ContextSeparator}${TOpt['context']}` : Key;
    export type TFunctionReturn<Ns extends Namespace, Key, TOpt extends TOptions, ActualNS extends Namespace = NsByTOptions<Ns, TOpt>, ActualKey = KeyWithContext<Key, TOpt>> = $IsResourcesDefined extends true ? ActualKey extends `${infer Nsp}${_NsSeparator}${infer RestKey}` ? ParseTReturn<RestKey, Resources[Nsp & keyof Resources], TOpt> : ParseTReturn<ActualKey, Resources[$FirstNamespace<ActualNS>], TOpt> : DefaultTReturn<TOpt>;
    export type TFunctionDetailedResult<T = string, TOpt extends TOptions = {}> = {
        /**
         * The plain used key
         */
        usedKey: string;
        /**
         * The translation result.
         */
        res: T;
        /**
         * The key with context / plural
         */
        exactUsedKey: string;
        /**
         * The used language for this translation.
         */
        usedLng: string;
        /**
         * The used namespace for this translation.
         */
        usedNS: string;
        /**
         * The parameters used for interpolation.
         */
        usedParams: InterpolationMap<T> & {
            count?: TOpt['count'];
        };
    };
    type TFunctionProcessReturnValue<Ret, DefaultValue> = Ret extends string | $SpecialObject | null ? Ret : [
        DefaultValue
    ] extends [
        never
    ] ? Ret : DefaultValue;
    type TFunctionReturnOptionalDetails<Ret, TOpt extends TOptions> = TOpt['returnDetails'] extends true ? TFunctionDetailedResult<Ret, TOpt> : Ret;
    type AppendKeyPrefix<Key, KPrefix> = KPrefix extends string ? `${KPrefix}${_KeySeparator}${Key & string}` : Key;
    /** ************************
     * T function declaration *
     ************************* */
    export interface TFunction<Ns extends Namespace = DefaultNamespace, KPrefix = undefined> {
        $TFunctionBrand: $IsResourcesDefined extends true ? `${$FirstNamespace<Ns>}` : never;
        <const Key extends ParseKeys<Ns, TOpt, KPrefix> | TemplateStringsArray, const TOpt extends TOptions, Ret extends TFunctionReturn<Ns, AppendKeyPrefix<Key, KPrefix>, TOpt>, const ActualOptions extends TOpt & InterpolationMap<Ret> = TOpt & InterpolationMap<Ret>, DefaultValue extends string = never>(...args: [
            key: Key | Key[],
            options?: ActualOptions
        ] | [
            key: string | string[],
            options: TOpt & $Dictionary & {
                defaultValue: DefaultValue;
            }
        ] | [
            key: string | string[],
            defaultValue: DefaultValue,
            options?: TOpt & $Dictionary
        ]): TFunctionReturnOptionalDetails<TFunctionProcessReturnValue<$NoInfer<Ret>, DefaultValue>, TOpt>;
    }
    export type KeyPrefix<Ns extends Namespace> = ResourceKeys<true>[$FirstNamespace<Ns>] | undefined;
}

declare module "iztro" {
    export * as data from "iztro/lib/data/index";
    export * as star from "iztro/lib/star/index";
    export * as util from "iztro/lib/utils/index";
    export * as astro from "iztro/lib/astro/index";
}

declare module "iztro/lib/astro/astro" {
    import { Config, Language, Option, Plugin } from "iztro/lib/data/types/index";
    import { EarthlyBranchName, GenderName, HeavenlyStemName } from "iztro/lib/i18n/index";
    import FunctionalAstrolabe from "iztro/lib/astro/FunctionalAstrolabe";
    /**
     * 批量加载插件
     *
     * @version v2.3.0
     *
     * @param plugins 插件方法数组
     */
    export const loadPlugins: (plugins: Plugin[]) => void;
    /**
     * 加载单个插件
     *
     * @version v2.3.0
     *
     * @param plugin 插件方法
     */
    export const loadPlugin: (plugin: Plugin) => void;
    /**
     * 全局配置四化和亮度
     *
     * 由于key和value都有可能是不同语言传进来的，
     * 所以需会将key和value转化为对应的i18n key。
     *
     * @version 2.3.0
     *
     * @param {Config} param0 自定义配置
     */
    export const config: ({ mutagens, brightness, yearDivide, ageDivide, horoscopeDivide, dayDivide, algorithm, }: Config) => void;
    export const getConfig: () => {
        mutagens: Partial<Record<"jiaHeavenly" | "yiHeavenly" | "bingHeavenly" | "dingHeavenly" | "wuHeavenly" | "jiHeavenly" | "gengHeavenly" | "xinHeavenly" | "renHeavenly" | "guiHeavenly", ("lianzhenMaj" | "pojunMaj" | "wuquMaj" | "taiyangMaj" | "tianjiMaj" | "tianliangMaj" | "ziweiMaj" | "taiyinMaj" | "tiantongMaj" | "wenchangMin" | "jumenMaj" | "tanlangMaj" | "youbiMin" | "wenquMin" | "zuofuMin" | "huoxingMin" | "tianxiangMaj" | "lucunMin" | "si" | "bing" | "tianfuMaj" | "qishaMaj" | "tianmaMin" | "qingyangMin" | "tuoluoMin" | "lingxingMin" | "tiankuiMin" | "tianyueMin" | "dikongMin" | "dijieMin" | "jieshaAdj" | "tiankong" | "tianxing" | "tianyao" | "jieshen" | "yinsha" | "tianxi" | "tianguan" | "tianfu" | "tianku" | "tianxu" | "longchi" | "fengge" | "hongluan" | "guchen" | "guasu" | "feilian" | "posui" | "taifu" | "fenggao" | "tianwu" | "tianyue" | "santai" | "bazuo" | "engguang" | "tiangui" | "tiancai" | "tianshou" | "jiekong" | "xunzhong" | "xunkong" | "kongwang" | "jielu" | "yuede" | "tianshang" | "tianshi" | "tianchu" | "changsheng" | "muyu" | "guandai" | "linguan" | "diwang" | "shuai" | "mu" | "jue" | "tai" | "yang" | "boshi" | "lishi" | "qinglong" | "xiaohao" | "jiangjun" | "zhoushu" | "faylian" | "xishen" | "bingfu" | "dahao" | "suipo" | "fubing" | "guanfu" | "suijian" | "huiqi" | "sangmen" | "guansuo" | "gwanfu" | "longde" | "baihu" | "tiande" | "diaoke" | "jiangxing" | "panan" | "suiyi" | "xiishen" | "huagai" | "jiesha" | "zhaisha" | "tiansha" | "zhibei" | "xianchi" | "yuesha" | "wangshen" | "yunkui" | "yunyue" | "yunchang" | "yunqu" | "yunluan" | "yunxi" | "yunlu" | "yunyang" | "yuntuo" | "yunma" | "liukui" | "liuyue" | "liuchang" | "liuqu" | "liuluan" | "liuxi" | "liulu" | "liuyang" | "liutuo" | "liuma" | "nianjie" | "yuekui" | "yueyue" | "yuechang" | "yuequ" | "yueluan" | "yuexi" | "yuelu" | "yueyang" | "yuetuo" | "yuema" | "rikui" | "riyue" | "richang" | "riqu" | "riluan" | "rixi" | "rilu" | "riyang" | "rituo" | "rima" | "shikui" | "shiyue" | "shichang" | "shiqu" | "shiluan" | "shixi" | "shilu" | "shiyang" | "shituo" | "shima")[]>>;
        brightness: Partial<Record<"lianzhenMaj" | "pojunMaj" | "wuquMaj" | "taiyangMaj" | "tianjiMaj" | "tianliangMaj" | "ziweiMaj" | "taiyinMaj" | "tiantongMaj" | "wenchangMin" | "jumenMaj" | "tanlangMaj" | "youbiMin" | "wenquMin" | "zuofuMin" | "huoxingMin" | "tianxiangMaj" | "lucunMin" | "si" | "bing" | "tianfuMaj" | "qishaMaj" | "tianmaMin" | "qingyangMin" | "tuoluoMin" | "lingxingMin" | "tiankuiMin" | "tianyueMin" | "dikongMin" | "dijieMin" | "jieshaAdj" | "tiankong" | "tianxing" | "tianyao" | "jieshen" | "yinsha" | "tianxi" | "tianguan" | "tianfu" | "tianku" | "tianxu" | "longchi" | "fengge" | "hongluan" | "guchen" | "guasu" | "feilian" | "posui" | "taifu" | "fenggao" | "tianwu" | "tianyue" | "santai" | "bazuo" | "engguang" | "tiangui" | "tiancai" | "tianshou" | "jiekong" | "xunzhong" | "xunkong" | "kongwang" | "jielu" | "yuede" | "tianshang" | "tianshi" | "tianchu" | "changsheng" | "muyu" | "guandai" | "linguan" | "diwang" | "shuai" | "mu" | "jue" | "tai" | "yang" | "boshi" | "lishi" | "qinglong" | "xiaohao" | "jiangjun" | "zhoushu" | "faylian" | "xishen" | "bingfu" | "dahao" | "suipo" | "fubing" | "guanfu" | "suijian" | "huiqi" | "sangmen" | "guansuo" | "gwanfu" | "longde" | "baihu" | "tiande" | "diaoke" | "jiangxing" | "panan" | "suiyi" | "xiishen" | "huagai" | "jiesha" | "zhaisha" | "tiansha" | "zhibei" | "xianchi" | "yuesha" | "wangshen" | "yunkui" | "yunyue" | "yunchang" | "yunqu" | "yunluan" | "yunxi" | "yunlu" | "yunyang" | "yuntuo" | "yunma" | "liukui" | "liuyue" | "liuchang" | "liuqu" | "liuluan" | "liuxi" | "liulu" | "liuyang" | "liutuo" | "liuma" | "nianjie" | "yuekui" | "yueyue" | "yuechang" | "yuequ" | "yueluan" | "yuexi" | "yuelu" | "yueyang" | "yuetuo" | "yuema" | "rikui" | "riyue" | "richang" | "riqu" | "riluan" | "rixi" | "rilu" | "riyang" | "rituo" | "rima" | "shikui" | "shiyue" | "shichang" | "shiqu" | "shiluan" | "shixi" | "shilu" | "shiyang" | "shituo" | "shima", ("li" | "wang" | "de" | "miao" | "ping" | "xian" | "bu")[]>>;
        yearDivide: "exact" | "normal";
        ageDivide: "normal" | "birthday";
        dayDivide: "forward" | "current";
        horoscopeDivide: "exact" | "normal";
        algorithm: "default" | "zhongzhou";
    };
    /**
     * 通过阳历获取星盘信息
     *
     * @deprecated 此方法已在`v2.0.5`废弃，请用 `bySolar` 方法替换，参数不变
     *
     * @param solarDateStr 阳历日期【YYYY-M-D】
     * @param timeIndex 出生时辰序号【0~12】
     * @param gender 性别【男|女】
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言
     * @returns 星盘信息
     */
    export function astrolabeBySolarDate<T extends FunctionalAstrolabe>(solarDateStr: string, timeIndex: number, gender: GenderName, fixLeap?: boolean, language?: Language): T;
    /**
     * 通过阳历获取星盘信息
     *
     * @param solarDate 阳历日期【YYYY-M-D】
     * @param timeIndex 出生时辰序号【0~12】
     * @param gender 性别【男|女】
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言
     * @returns 星盘信息
     */
    export function bySolar<T extends FunctionalAstrolabe>(solarDate: string, timeIndex: number, gender: GenderName, fixLeap?: boolean, language?: Language): T;
    /**
     * 通过农历获取星盘信息
     *
     * @deprecated 此方法已在`v2.0.5`废弃，请用 `byLunar` 方法替换，参数不变
     *
     * @param lunarDateStr 农历日期【YYYY-M-D】，例如2000年七月十七则传入 2000-7-17
     * @param timeIndex 出生时辰序号【0~12】
     * @param gender 性别【男|女】
     * @param isLeapMonth 是否闰月【默认 false】，当实际月份没有闰月时该参数不生效
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言
     * @returns 星盘数据
     */
    export function astrolabeByLunarDate<T extends FunctionalAstrolabe>(lunarDateStr: string, timeIndex: number, gender: GenderName, isLeapMonth?: boolean, fixLeap?: boolean, language?: Language): T;
    /**
     * 通过农历获取星盘信息
     *
     * @param lunarDateStr 农历日期【YYYY-M-D】，例如2000年七月十七则传入 2000-7-17
     * @param timeIndex 出生时辰序号【0~12】
     * @param gender 性别【男|女】
     * @param isLeapMonth 是否闰月【默认 false】，当实际月份没有闰月时该参数不生效
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言
     * @returns 星盘数据
     */
    export function byLunar<T extends FunctionalAstrolabe>(lunarDateStr: string, timeIndex: number, gender: GenderName, isLeapMonth?: boolean, fixLeap?: boolean, language?: Language): T;
    export function rearrangeAstrolable<T extends FunctionalAstrolabe>({ from, astrolable, option, }: {
        from: {
            heavenlyStem: HeavenlyStemName;
            earthlyBranch: EarthlyBranchName;
        };
        astrolable: T;
        option: Option;
    }): T;
    /**
     * 获取排盘信息。
     *
     * @param param0 排盘参数
     * @returns 星盘信息
     */
    export function withOptions<T extends FunctionalAstrolabe>(option: Option): T;
    /**
     * 通过公历获取十二生肖
     *
     * @version v1.2.1
     *
     * @param solarDateStr 阳历日期【YYYY-M-D】
     * @param language 输出语言，默认为中文
     * @returns 十二生肖
     */
    export const getZodiacBySolarDate: (solarDateStr: string, language?: Language) => string;
    /**
     * 通过阳历获取星座
     *
     * @version v1.2.1
     *
     * @param solarDateStr 阳历日期【YYYY-M-D】
     * @param language 输出语言，默认为中文
     * @returns 星座
     */
    export const getSignBySolarDate: (solarDateStr: string, language?: Language) => string;
    /**
     * 通过农历获取星座
     *
     * @version v1.2.1
     *
     * @param lunarDateStr 农历日期【YYYY-M-D】
     * @param isLeapMonth 是否闰月，如果该月没有闰月则此字段不生效
     * @param language 输出语言，默认为中文
     * @returns 星座
     */
    export const getSignByLunarDate: (lunarDateStr: string, isLeapMonth?: boolean, language?: Language) => string;
    /**
     * 通过阳历获取命宫主星
     *
     * @version v1.2.1
     *
     * @param solarDateStr 阳历日期【YYYY-M-D】
     * @param timeIndex 出生时辰序号【0~12】
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言，默认为中文
     * @returns 命宫主星
     */
    export const getMajorStarBySolarDate: (solarDateStr: string, timeIndex: number, fixLeap?: boolean, language?: Language) => string;
    /**
     * 通过农历获取命宫主星
     *
     * @version v1.2.1
     *
     * @param lunarDateStr 农历日期【YYYY-M-D】，例如2000年七月十七则传入 2000-7-17
     * @param timeIndex 出生时辰序号【0~12】
     * @param isLeapMonth 是否闰月，如果该月没有闰月则此字段不生效
     * @param fixLeap 是否调整闰月情况【默认 true】，假入调整闰月，则闰月的前半个月算上个月，后半个月算下个月
     * @param language 输出语言，默认为中文
     * @returns 命宫主星
     */
    export const getMajorStarByLunarDate: (lunarDateStr: string, timeIndex: number, isLeapMonth?: boolean, fixLeap?: boolean, language?: Language) => string;
}

declare module "iztro/lib/astro/FunctionalAstrolabe" {
    import { Astrolabe, Plugin } from "iztro/lib/data/types/index";
    import { EarthlyBranchName, PalaceName, StarName } from "iztro/lib/i18n/index";
    import { IFunctionalStar } from "iztro/lib/star/FunctionalStar";
    import { IFunctionalPalace } from "iztro/lib/astro/FunctionalPalace";
    import { IFunctionalSurpalaces } from "iztro/lib/astro/FunctionalSurpalaces";
    import { IFunctionalHoroscope } from "iztro/lib/astro/FunctionalHoroscope";
    /**
     * 星盘类接口定义。
     *
     * 文档地址：https://docs.iztro.com/posts/astrolabe.html#functionalastrolabe
     */
    export interface IFunctionalAstrolabe extends Astrolabe {
        /**
         * 插件注入方法
         *
         * @version v2.3.0
         *
         * @param plugin 插件函数
         */
        use(plugin: Plugin): void;
        /**
         * 获取运限数据
         *
         * @version v0.2.0
         *
         * @param date 阳历日期【可选】，默认为调用时的日期
         * @param timeIndex 时辰索引【可选】，默认会自动读取当前时间的时辰
         * @returns 运限数据
         */
        horoscope: (date?: string | Date, timeIndex?: number) => IFunctionalHoroscope;
        /**
         * 通过星耀名称获取到当前星耀的对象实例
         *
         * @version v1.2.0
         *
         * @param starName 星耀名称
         * @returns 星耀实例
         */
        star: (starName: StarName) => IFunctionalStar;
        /**
         * 获取星盘的某一个宫位
         *
         * @version v1.0.0
         *
         * @param indexOrName 宫位索引或者宫位名称
         * @returns 对应的宫位数据，若没有找到则返回undefined
         */
        palace: (indexOrName: number | PalaceName) => IFunctionalPalace | undefined;
        /**
         * 获取三方四正宫位，所谓三方四正就是传入的目标宫位，以及其对宫，财帛位和官禄位，总共四个宫位
         *
         * @version v1.1.0
         *
         * @param indexOrName 宫位索引或者宫位名称
         * @returns 三方四正宫位
         */
        surroundedPalaces: (indexOrName: number | PalaceName) => IFunctionalSurpalaces;
        /**
         *
         * 判断某一个宫位三方四正是否包含目标星耀，必须要全部包含才会返回true
         *
         * @version v1.0.0
         *
         * @param indexOrName 宫位索引或者宫位名称
         * @param stars 星耀名称数组
         * @returns true | false
         */
        isSurrounded: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
        /**
         * 判断三方四正内是否有传入星耀的其中一个，只要命中一个就会返回true
         *
         * @version v1.1.0
         * @deprecated v1.2.0
         *
         * @param indexOrName 宫位索引或者宫位名称
         * @param stars 星耀名称数组
         * @returns true | false
         */
        isSurroundedOneOf: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
        /**
         * 判断某一个宫位三方四正是否不含目标星耀，必须要全部都不在三方四正内含才会返回true
         *
         * @version v1.1.0
         * @deprecated v1.2.0
         *
         * @param indexOrName 宫位索引或者宫位名称
         * @param stars 星耀名称数组
         * @returns true | false
         */
        notSurrounded: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
    }
    /**
     * 星盘类。
     *
     * 文档地址：https://docs.iztro.com/posts/astrolabe.html#functionalastrolabe
     */
    export default class FunctionalAstrolabe implements IFunctionalAstrolabe {
        gender: string;
        solarDate: string;
        lunarDate: string;
        chineseDate: string;
        rawDates: {
            lunarDate: import("lunar-lite/lib/types").LunarDate;
            chineseDate: import("lunar-lite/lib/types").HeavenlyStemAndEarthlyBranchDate;
        };
        time: string;
        timeRange: string;
        sign: string;
        zodiac: string;
        earthlyBranchOfSoulPalace: EarthlyBranchName;
        earthlyBranchOfBodyPalace: EarthlyBranchName;
        soul: StarName;
        body: StarName;
        fiveElementsClass: import("iztro/lib/i18n/index").FiveElementsClassName;
        palaces: IFunctionalPalace[];
        copyright: string;
        private plugins;
        constructor(data: Astrolabe);
        use(plugin: Plugin): void;
        star: (starName: StarName) => IFunctionalStar;
        horoscope: (targetDate?: string | Date, timeIndexOfTarget?: number) => IFunctionalHoroscope;
        palace: (indexOrName: number | PalaceName) => IFunctionalPalace | undefined;
        surroundedPalaces: (indexOrName: number | PalaceName) => IFunctionalSurpalaces;
        /**
         * @deprecated 此方法已在`v1.2.0`废弃，请用下列方法替换
         *
         * @example
         *  // AS IS
         *  astrolabe.isSurrounded(0, ["紫微"]);
         *
         *  // TO BE
         *  astrolabe.surroundedPalaces(0).have(["紫微"]);
         */
        isSurrounded: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
        /**
         * @deprecated 此方法已在`v1.2.0`废弃，请用下列方法替换
         *
         * @example
         *  // AS IS
         *  astrolabe.isSurroundedOneOf(0, ["紫微"]);
         *
         *  // TO BE
         *  astrolabe.surroundedPalaces(0).haveOneOf(["紫微"]);
         */
        isSurroundedOneOf: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
        /**
         * @deprecated 此方法已在`v1.2.0`废弃，请用下列方法替换
         *
         * @example
         *  // AS IS
         *  astrolabe.notSurrounded(0, ["紫微"]);
         *
         *  // TO BE
         *  astrolabe.surroundedPalaces(0).notHave(["紫微"]);
         */
        notSurrounded: (indexOrName: number | PalaceName, stars: StarName[]) => boolean;
    }
}

declare module "iztro/lib/astro/FunctionalHoroscope" {
    import { Horoscope, Scope } from "iztro/lib/data/types/index";
    import { Mutagen, PalaceName, StarName } from "iztro/lib/i18n/index";
    import { IFunctionalAstrolabe } from "iztro/lib/astro/FunctionalAstrolabe";
    import { IFunctionalSurpalaces } from "iztro/lib/astro/FunctionalSurpalaces";
    import { IFunctionalPalace } from "iztro/lib/astro/FunctionalPalace";
    export interface IFunctionalHoroscope extends Horoscope {
        astrolabe: IFunctionalAstrolabe;
        /**
         * 获取小限宫位
         *
         * @version v1.3.0
         *
         * @returns {IFunctionalPalace | undefined} 小限宫位
         */
        agePalace: () => IFunctionalPalace | undefined;
        /**
         * 获取运限宫位
         *
         * @version v1.3.0
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @returns {IFunctionalPalace | undefined} 指定宫位
         */
        palace: (palaceName: PalaceName, scope: Scope) => IFunctionalPalace | undefined;
        /**
         * 获取运限指定宫位的三方四正宫位
         *
         * @version v1.3.0
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @returns {IFunctionalSurpalaces | undefined} 指定宫位的三方四正
         */
        surroundPalaces: (palaceName: PalaceName, scope: Scope) => IFunctionalSurpalaces | undefined;
        /**
         * 判断在指定运限的宫位内是否包含流耀，需要全部包含才返回true
         *
         * @version v1.3.0
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @param horoscopeStar 流耀
         * @returns {boolean} 是否包含指定流耀
         */
        hasHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscopeStar: StarName[]) => boolean;
        /**
         * 判断指定运限宫位内是否不含流耀，需要全部不包含才返回true
         *
         * @version v1.3.2
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @param horoscope 流耀
         * @returns {boolean} 是否不含指定流耀
         */
        notHaveHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscope: StarName[]) => boolean;
        /**
         * 判断指定运限宫位内是否含有指定流耀，只要包含其中一颗就返回true
         *
         * @version v1.3.3
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @param horoscope 流耀
         * @returns {boolean} 是否含有（部分）指定流耀中
         */
        hasOneOfHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscopeStar: StarName[]) => boolean;
        /**
         * 判断指定运限宫位内是否存在运限四化
         *
         * @version v1.3.4
         *
         * @param palaceName 宫位名称
         * @param scope 指定获取哪个运限的宫位
         * @param horoscopeMutagen 运限四化
         * @returns {boolean} 是否含有运限四化
         */
        hasHoroscopeMutagen: (palaceName: PalaceName, scope: Scope, horoscopeMutagen: Mutagen) => boolean;
    }
    export default class FunctionalHoroscope implements IFunctionalHoroscope {
        lunarDate: string;
        solarDate: string;
        decadal: import("iztro/lib/data/types/index").HoroscopeItem;
        age: import("iztro/lib/data/types/index").HoroscopeItem & {
            nominalAge: number;
        };
        yearly: import("iztro/lib/data/types/index").HoroscopeItem & {
            yearlyDecStar: {
                jiangqian12: StarName[];
                suiqian12: StarName[];
            };
        };
        monthly: import("iztro/lib/data/types/index").HoroscopeItem;
        daily: import("iztro/lib/data/types/index").HoroscopeItem;
        hourly: import("iztro/lib/data/types/index").HoroscopeItem;
        astrolabe: IFunctionalAstrolabe;
        constructor(data: Horoscope, astrolabe: IFunctionalAstrolabe);
        agePalace: () => IFunctionalPalace | undefined;
        palace: (palaceName: PalaceName, scope: Scope) => IFunctionalPalace | undefined;
        surroundPalaces: (palaceName: PalaceName, scope: Scope) => IFunctionalSurpalaces;
        hasHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscopeStar: StarName[]) => boolean;
        notHaveHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscopeStar: StarName[]) => boolean;
        hasOneOfHoroscopeStars: (palaceName: PalaceName, scope: Scope, horoscopeStar: StarName[]) => boolean;
        hasHoroscopeMutagen: (palaceName: PalaceName, scope: Scope, horoscopeMutagen: Mutagen) => boolean;
    }
}

declare module "iztro/lib/astro/FunctionalPalace" {
    import { Palace } from "iztro/lib/data/types/index";
    import { Mutagen, PalaceName, StarName } from "iztro/lib/i18n/index";
    import { IFunctionalAstrolabe } from "iztro/lib/astro/FunctionalAstrolabe";
    /**
     * 宫位类的接口定义。
     *
     * 文档地址：https://docs.iztro.com/posts/palace.html#functionalastrolabe
     */
    export interface IFunctionalPalace extends Palace {
        /**
         * 判断某个宫位内是否有传入的星耀，要所有星耀都在宫位内才会返回true
         *
         * @version v1.0.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returns true | false
         */
        has: (stars: StarName[]) => boolean;
        /**
         * 判断某个宫位内是否有传入的星耀，要所有星耀都不在宫位内才会返回true
         *
         * @version v1.0.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returnstrue | false
         */
        notHave: (stars: StarName[]) => boolean;
        /**
         * 判断某个宫位内是否有传入星耀的其中一个，只要命中一个就会返回true
         *
         * @version v1.0.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returns true | false
         */
        hasOneOf: (stars: StarName[]) => boolean;
        /**
         * 判断宫位内是否有生年四化
         *
         * @version v1.2.0
         *
         * @param mutagen 四化名称【禄｜权｜科｜忌】
         * @returns true | false
         */
        hasMutagen: (mutagen: Mutagen) => boolean;
        /**
         * 判断宫位内是否没有生年四化
         *
         * @version v1.2.0
         *
         * @param mutagen 四化名称【禄｜权｜科｜忌】
         * @returns true | false
         */
        notHaveMutagen: (mutagen: Mutagen) => boolean;
        /**
         * 判断一个宫位是否为空宫（没有主星），
         * 有些派别在宫位内有某些星耀的情况下，
         * 是不会将该宫位判断为空宫的。
         * 所以加入一个参数来传入星耀。
         *
         * @version v2.0.6
         *
         * @param excludeStars 星耀名称数组
         *
         * @returns {boolean}  true | false
         */
        isEmpty: (excludeStars?: StarName[]) => boolean;
        /**
         * 给宫位设置星盘对象
         *
         * @version v2.1.0
         *
         * @param astro 星盘对象
         * @returns {void}
         */
        setAstrolabe: (astro: IFunctionalAstrolabe) => void;
        /**
         * 获取当前宫位所在的星盘对象
         *
         * @version v2.1.0
         *
         * @returns {IFunctionalAstrolabe}
         */
        astrolabe: () => IFunctionalAstrolabe | undefined;
        /**
         * 判断是否从源宫位飞化到目标宫位，四化可传入一个数组或者一个字符串，传入四化全部飞化到目标宫位即返回true
         *
         * @version v2.1.0
         *
         * @param  to 目标宫位
         * @param withMutagens 四化（禄、权、科、忌）
         * @returns {boolean}
         */
        fliesTo: (to: number | PalaceName, withMutagens: Mutagen | Mutagen[]) => boolean;
        /**
         * 判断是否从源宫位飞化其中一颗四化星到目标宫位，传入四化只要有一颗飞化到目标宫位即返回true
         *
         * @version v2.1.0
         *
         * @param  to 目标宫位
         * @param withMutagens 四化（禄、权、科、忌）
         * @returns {boolean}
         */
        fliesOneOfTo: (to: number | PalaceName, withMutagens: Mutagen[]) => boolean;
        /**
         * 判断是否没有从源宫位飞化到目标宫位，四化可传入一个数组或者一个字符串，传入四化全部没有飞化到目标宫位才返回true
         *
         * @version v2.1.0
         *
         * @param to 目标宫位
         * @param withMutagens 四化（禄、权、科、忌）
         * @returns {boolean}
         */
        notFlyTo: (to: number | PalaceName, withMutagens: Mutagen | Mutagen[]) => boolean;
        /**
         * 判断宫位是否有自化，传入四化数组时需要全部满足才返回true
         *
         * @version v2.1.0
         *
         * @param withMutagens 四化（禄、权、科、忌）
         * @returns {boolean}
         */
        selfMutaged: (withMutagens: Mutagen | Mutagen[]) => boolean;
        /**
         * 判断宫位是否有自化，若不传入参数则会判断所有四化，满足一颗即返回true
         *
         * @version v2.1.0
         *
         * @param withMutagens 四化（禄、权、科、忌）
         * @returns {boolean}
         */
        selfMutagedOneOf: (withMutagens?: Mutagen[]) => boolean;
        /**
         * 判断宫位是否有自化，如果传入参数，则只判断传入的四化是否有自化，否则将会判断所有四化
         *
         * @version v2.1.0
         *
         * @param withMutagens 【可选】四化（禄、权、科、忌）
         * @returns {boolean}
         */
        notSelfMutaged: (withMutagens?: Mutagen | Mutagen[]) => boolean;
        /**
         * 获取当前宫位产生四化的4个宫位数组，下标分别对【禄，权，科，忌】
         *
         * @version v2.1.0
         *
         * @returns {(IFunctionalPalace | undefined)[]}
         */
        mutagedPlaces: () => (IFunctionalPalace | undefined)[];
    }
    /**
     * 宫位类。
     *
     * 文档地址：https://docs.iztro.com/posts/palace.html#functionalastrolabe
     */
    export default class FunctionalPalace implements IFunctionalPalace {
        private _astrolabe?;
        index: number;
        name: PalaceName;
        isBodyPalace: boolean;
        isOriginalPalace: boolean;
        heavenlyStem: import("iztro/lib/i18n/index").HeavenlyStemName;
        earthlyBranch: import("iztro/lib/i18n/index").EarthlyBranchName;
        majorStars: import("iztro/lib/star/FunctionalStar").default[];
        minorStars: import("iztro/lib/star/FunctionalStar").default[];
        adjectiveStars: import("iztro/lib/star/FunctionalStar").default[];
        changsheng12: StarName;
        boshi12: StarName;
        jiangqian12: StarName;
        suiqian12: StarName;
        decadal: import("iztro/lib/data/types/index").Decadal;
        ages: number[];
        constructor(data: Palace);
        has: (stars: StarName[]) => boolean;
        notHave: (stars: StarName[]) => boolean;
        hasOneOf: (stars: StarName[]) => boolean;
        hasMutagen: (mutagen: Mutagen) => boolean;
        notHaveMutagen: (mutagen: Mutagen) => boolean;
        isEmpty: (excludeStars?: StarName[]) => boolean;
        setAstrolabe: (astro: IFunctionalAstrolabe) => IFunctionalAstrolabe;
        astrolabe: () => IFunctionalAstrolabe | undefined;
        fliesTo: (to: number | PalaceName, withMutagens: Mutagen | Mutagen[]) => boolean;
        fliesOneOfTo: (to: number | PalaceName, withMutagens: Mutagen[]) => boolean;
        notFlyTo: (to: number | PalaceName, withMutagens: Mutagen | Mutagen[]) => boolean;
        selfMutaged: (withMutagens: Mutagen | Mutagen[]) => boolean;
        selfMutagedOneOf: (withMutagens?: Mutagen[]) => boolean;
        notSelfMutaged: (withMutagens?: Mutagen | Mutagen[]) => boolean;
        mutagedPlaces: () => (IFunctionalPalace | undefined)[];
    }
}

declare module "iztro/lib/astro/FunctionalSurpalaces" {
    import { SurroundedPalaces } from "iztro/lib/data/types/index";
    import { Mutagen, StarName } from "iztro/lib/i18n/index";
    export interface IFunctionalSurpalaces extends SurroundedPalaces {
        /**
         * 判断某一个宫位三方四正是否包含目标星耀，必须要全部包含才会返回true
         *
         * @version v1.2.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returns true | false
         */
        have: (stars: StarName[]) => boolean;
        /**
         * 判断某一个宫位三方四正是否不含目标星耀，必须要全部都不在三方四正内含才会返回true
         *
         * @version v1.2.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returnstrue | false
         */
        notHave: (stars: StarName[]) => boolean;
        /**
         * 判断三方四正内是否有传入星耀的其中一个，只要命中一个就会返回true
         *
         * @version v1.2.0
         *
         * @param stars 星耀名称，可以包含主星、辅星、杂耀
         * @returns true | false
         */
        haveOneOf: (stars: StarName[]) => boolean;
        /**
         * 判断某一个宫位三方四正是否有四化
         *
         * @version v1.2.0
         *
         * @param mutagen 四化名称【禄｜权｜科｜忌】
         * @returns true | false
         */
        haveMutagen: (mutagen: Mutagen) => boolean;
        /**
         * 判断某一个宫位三方四正是否没有四化
         *
         * @version v1.2.0
         *
         * @param mutagen 四化名称【禄｜权｜科｜忌】
         * @returns true | false
         */
        notHaveMutagen: (mutagen: Mutagen) => boolean;
    }
    export class FunctionalSurpalaces implements IFunctionalSurpalaces {
        target: import("iztro/lib/astro/FunctionalPalace").IFunctionalPalace;
        opposite: import("iztro/lib/astro/FunctionalPalace").IFunctionalPalace;
        wealth: import("iztro/lib/astro/FunctionalPalace").IFunctionalPalace;
        career: import("iztro/lib/astro/FunctionalPalace").IFunctionalPalace;
        constructor({ target, opposite, wealth, career }: SurroundedPalaces);
        have: (stars: StarName[]) => boolean;
        notHave: (stars: StarName[]) => boolean;
        haveOneOf: (stars: StarName[]) => boolean;
        haveMutagen: (mutagen: Mutagen) => boolean;
        notHaveMutagen: (mutagen: Mutagen) => boolean;
    }
}

declare module "iztro/lib/astro/index" {
    export * from "iztro/lib/astro/palace";
    export * from "iztro/lib/astro/astro";
}

declare module "iztro/lib/astro/palace" {
    import { SoulAndBody, Decadal, AstrolabeParam } from "iztro/lib/data/types/index";
    import { EarthlyBranchName, FiveElementsClassName, HeavenlyStemName, PalaceName } from "iztro/lib/i18n/index";
    /**
     * 获取命宫以及身宫数据
     *
     * 1. 定寅首
     * - 甲己年生起丙寅，乙庚年生起戊寅，
     * - 丙辛年生起庚寅，丁壬年生起壬寅，
     * - 戊癸年生起甲寅。
     *
     * 2. 安命身宫诀
     * - 寅起正月，顺数至生月，逆数生时为命宫。
     * - 寅起正月，顺数至生月，顺数生时为身宫。
     *
     * @param {AstrolabeParam} param 通用排盘参数
     * @returns {SoulAndBody} 命宫和身宫数据
     */
    export const getSoulAndBody: (param: AstrolabeParam) => SoulAndBody;
    /**
     * 定五行局法（以命宫天干地支而定）
     *
     * 纳音五行计算取数巧记口诀：
     *
     * - 甲乙丙丁一到五，子丑午未一来数，
     * - 寅卯申酉二上走，辰巳戌亥三为足。
     * - 干支相加多减五，五行木金水火土。
     *
     * 注解：
     *
     * 1、五行取数：木1 金2 水3 火4 土5
     *
     *  天干取数：
     *  - 甲乙 ——> 1
     *  - 丙丁 ——> 2
     *  - 戊己 ——> 3
     *  - 庚辛 ——> 4
     *  - 壬癸 ——> 5
     *
     *  地支取数：
     *  - 子午丑未 ——> 1
     *  - 寅申卯酉 ——> 2
     *  - 辰戌巳亥 ——> 3
     *
     * 2、计算方法：
     *
     *  干支数相加，超过5者减去5，以差论之。
     *  - 若差为1则五行属木
     *  - 若差为2则五行属金
     *  - 若差为3则五行属水
     *  - 若差为4则五行属火
     *  - 若差为5则五行属土
     *
     * 3、举例：
     *  - 丙子：丙2 子1=3 ——> 水 ——> 水二局
     *  - 辛未：辛4 未1=5 ——> 土 ——> 土五局
     *  - 庚申：庚4 申2=6 ——> 6-5=1 ——> 木 ——> 木三局
     *
     * @param heavenlyStemName 天干
     * @param earthlyBranchName 地支
     * @returns 水二局 ｜ 木三局 ｜ 金四局 ｜ 土五局 ｜ 火六局
     */
    export const getFiveElementsClass: (heavenlyStemName: HeavenlyStemName, earthlyBranchName: EarthlyBranchName) => FiveElementsClassName;
    /**
     * 获取从寅宫开始的各个宫名
     *
     * @param fromIndex 命宫索引
     * @returns 从寅宫开始的各个宫名
     */
    export const getPalaceNames: (fromIndex: number) => PalaceName[];
    /**
     * 起大限
     *
     * - 大限由命宫起，阳男阴女顺行；
     * - 阴男阳女逆行，每十年过一宫限。
     *
     * @param solarDateStr 公历日期
     * @param timeIndex 出生时索引
     * @param gender 性别
     * @param fixLeap 是否修正闰月，若修正，则闰月前15天按上月算，后15天按下月算
     * @returns 从寅宫开始的大限年龄段
     */
    export const getHoroscope: (param: AstrolabeParam) => {
        decadals: Decadal[];
        ages: number[][];
    };
}

declare module "iztro/lib/data/constants" {
    /** 支持的语言 */
    export const LANGUAGES: string[];
    /** 十天干 */
    export const HEAVENLY_STEMS: readonly [
        "jiaHeavenly",
        "yiHeavenly",
        "bingHeavenly",
        "dingHeavenly",
        "wuHeavenly",
        "jiHeavenly",
        "gengHeavenly",
        "xinHeavenly",
        "renHeavenly",
        "guiHeavenly"
    ];
    /** 十二地支 */
    export const EARTHLY_BRANCHES: readonly [
        "ziEarthly",
        "chouEarthly",
        "yinEarthly",
        "maoEarthly",
        "chenEarthly",
        "siEarthly",
        "wuEarthly",
        "weiEarthly",
        "shenEarthly",
        "youEarthly",
        "xuEarthly",
        "haiEarthly"
    ];
    /** 十二生肖（按地支顺序） */
    export const ZODIAC: readonly [
        "rat",
        "ox",
        "tiger",
        "rabbit",
        "dragon",
        "snake",
        "horse",
        "sheep",
        "monkey",
        "rooster",
        "dog",
        "pig"
    ];
    /** 紫微斗数十二宫名称 */
    export const PALACES: readonly [
        "soulPalace",
        "parentsPalace",
        "spiritPalace",
        "propertyPalace",
        "careerPalace",
        "friendsPalace",
        "surfacePalace",
        "healthPalace",
        "wealthPalace",
        "childrenPalace",
        "spousePalace",
        "siblingsPalace"
    ];
    /** 性别对应阴阳，男为阳，女为阴 */
    export const GENDER: {
        readonly male: "阳";
        readonly female: "阴";
    };
    /**
     * 五行局，用于定紫微星和算起运年龄
     * 几局就从几岁（虚岁）开始起运
     * 比如 木三局 就从3岁开始起运
     *
     * @enum
     *  - 2 水二局
     *  - 3 木三局
     *  - 4 金四局
     *  - 5 土五局
     *  - 6 火六局
     */
    export enum FiveElementsClass {
        water2nd = 2,
        wood3rd = 3,
        metal4th = 4,
        earth5th = 5,
        fire6th = 6
    }
    /**
     * 时辰, 子时分早晚。
     * 其中 00:00-01:00 为早子时，23:00-00:00 为晚子时
     */
    export const CHINESE_TIME: readonly [
        "earlyRatHour",
        "oxHour",
        "tigerHour",
        "rabbitHour",
        "dragonHour",
        "snakeHour",
        "horseHour",
        "goatHour",
        "monkeyHour",
        "roosterHour",
        "dogHour",
        "pigHour",
        "lateRatHour"
    ];
    /** 时辰序号所对应的时间段，与 `CHINESE_TIME` 一一对应 */
    export const TIME_RANGE: readonly [
        "00:00~01:00",
        "01:00~03:00",
        "03:00~05:00",
        "05:00~07:00",
        "07:00~09:00",
        "09:00~11:00",
        "11:00~13:00",
        "13:00~15:00",
        "15:00~17:00",
        "17:00~19:00",
        "19:00~21:00",
        "21:00~23:00",
        "23:00~00:00"
    ];
    /**
     * 五虎遁 从年干算月干。
     *
     * “五虎遁元”年上起月法，简称 `五虎遁`。
     * 因为正月建寅，所以正月的地支为寅，寅属虎，所以叫五虎盾。
     *
     * - 甲己之年丙作首
     * - 乙庚之岁戊为头
     * - 丙辛必定寻庚起
     * - 丁壬壬位顺行流
     * - 若问戊癸何方发
     * - 甲寅之上好追求
     */
    export const TIGER_RULE: {
        readonly jiaHeavenly: "bingHeavenly";
        readonly yiHeavenly: "wuHeavenly";
        readonly bingHeavenly: "gengHeavenly";
        readonly dingHeavenly: "renHeavenly";
        readonly wuHeavenly: "jiaHeavenly";
        readonly jiHeavenly: "bingHeavenly";
        readonly gengHeavenly: "wuHeavenly";
        readonly xinHeavenly: "gengHeavenly";
        readonly renHeavenly: "renHeavenly";
        readonly guiHeavenly: "jiaHeavenly";
    };
    /**
     * 五鼠遁 以日干算时干。
     *
     * “五鼠遁元”日上起时法，简称 `五鼠遁`。
     * 因为日支全部以“子”时打头来排列的，子为鼠，所以叫五鼠遁。
     *
     * - 甲己还加甲，乙庚丙作初。
     * - 丙辛从戊起，丁壬庚子居。
     * - 戊癸起壬子，周而复始求。
     */
    export const RAT_RULE: {
        readonly jiaHeavenly: "jiaHeavenly";
        readonly yiHeavenly: "bingHeavenly";
        readonly bingHeavenly: "wuHeavenly";
        readonly dingHeavenly: "gengHeavenly";
        readonly wuHeavenly: "renHeavenly";
        readonly jiHeavenly: "jiaHeavenly";
        readonly gengHeavenly: "bingHeavenly";
        readonly xinHeavenly: "wuHeavenly";
        readonly renHeavenly: "gengHeavenly";
        readonly guiHeavenly: "renHeavenly";
    };
}

declare module "iztro/lib/data/earthlyBranches" {
    /**
     * 十二地支信息
     * 其中包含：
     * 1. 阴阳（yinYang）
     * 2. 五行（fiveElements）
     * 3. 六冲（crash）
     *    - 子午相冲
     *    - 丑未相冲
     *    - 寅申相冲
     *    - 卯酉相冲
     *    - 辰戌相冲
     *    - 巳亥相冲
     * 4. 紫微斗数命主（soul）
     *    - 命主以命宫所在宫位地支定之
     * 5. 紫微斗数身主（body）
     *    - 身主以生年年支定之
     * 6. 身体部位【内】（inside）
     * 7. 身体部位【外】（outside）
     * 8. 健康提示（healthTip）
     */
    export const earthlyBranches: {
        readonly ziEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "水";
            readonly crash: "wuEarthly";
            readonly soul: "tanlangMaj";
            readonly body: "huoxingMin";
            readonly inside: "胆";
            readonly outside: "下体";
            readonly healthTip: "生殖系统、膀胱、尿道之疾病，听觉障碍";
        };
        readonly chouEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "土";
            readonly crash: "weiEarthly";
            readonly soul: "jumenMaj";
            readonly body: "tianxiangMaj";
            readonly inside: "肝";
            readonly outside: "小腿、脚（右）";
            readonly healthTip: "胸部、肋膜炎、胃病、脚部";
        };
        readonly yinEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "木";
            readonly crash: "shenEarthly";
            readonly soul: "lucunMin";
            readonly body: "tianliangMaj";
            readonly inside: "肺";
            readonly outside: "大腿（右）";
            readonly healthTip: "胆囊、关节、胫部、神经痛、风湿";
        };
        readonly maoEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "木";
            readonly crash: "youEarthly";
            readonly soul: "wenquMin";
            readonly body: "tiantongMaj";
            readonly inside: "大肠";
            readonly outside: "腰（右）、背";
            readonly healthTip: "肝病、颜面神经、失眠、神经衰弱";
        };
        readonly chenEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "土";
            readonly crash: "xuEarthly";
            readonly soul: "lianzhenMaj";
            readonly body: "wenchangMin";
            readonly inside: "胃";
            readonly outside: "胸、胳膊（右）";
            readonly healthTip: "消化系统、脊椎、皮肤疾病";
        };
        readonly siEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "火";
            readonly crash: "haiEarthly";
            readonly soul: "wuquMaj";
            readonly body: "tianjiMaj";
            readonly inside: "脾";
            readonly outside: "左肩";
            readonly healthTip: "喉头、牙病、感冒";
        };
        readonly wuEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "火";
            readonly crash: "ziEarthly";
            readonly soul: "pojunMaj";
            readonly body: "huoxingMin";
            readonly inside: "心";
            readonly outside: "头";
            readonly healthTip: "心脏、视觉、味觉障碍、火难";
        };
        readonly weiEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "土";
            readonly crash: "chouEarthly";
            readonly soul: "wuquMaj";
            readonly body: "tianxiangMaj";
            readonly inside: "小肠";
            readonly outside: "脸";
            readonly healthTip: "消化系统、胰脏、健忘症、疲倦、手腕、嘴唇";
        };
        readonly shenEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "金";
            readonly crash: "yinEarthly";
            readonly soul: "lianzhenMaj";
            readonly body: "tianliangMaj";
            readonly inside: "膀胱";
            readonly outside: "胸、胳膊（左）";
            readonly healthTip: "呼吸系统、肺部、消化系统、大肠";
        };
        readonly youEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "金";
            readonly crash: "maoEarthly";
            readonly soul: "wenquMin";
            readonly body: "tiantongMaj";
            readonly inside: "肾";
            readonly outside: "腰（左）、腹";
            readonly healthTip: "吐血、痢血、小肠之疾、脑出血、头腕部";
        };
        readonly xuEarthly: {
            readonly yinYang: "阳";
            readonly fiveElements: "土";
            readonly crash: "chenEarthly";
            readonly soul: "lucunMin";
            readonly body: "wenchangMin";
            readonly inside: "心包";
            readonly outside: "大腿（左）";
            readonly healthTip: "下半身之疾、子宫、痔疮、脚部";
        };
        readonly haiEarthly: {
            readonly yinYang: "阴";
            readonly fiveElements: "水";
            readonly crash: "siEarthly";
            readonly soul: "jumenMaj";
            readonly body: "tianjiMaj";
            readonly inside: "三焦";
            readonly outside: "小腿、脚（左）";
            readonly healthTip: "排泄机能障碍、肾脏、尿道、偏头痛";
        };
    };
}

declare module "iztro/lib/data/heavenlyStems" {
    /**
     * 十天干信息
     * 其中包含：
     * 1. 阴阳（yinYang）
     * 2. 五行（fiveElements）
     * 3. 天干相冲（crash）
     *    - 甲庚相冲
     *    - 乙辛相冲
     *    - 壬丙相冲
     *    - 癸丁相冲
     *    - 戊己土居中央，故无冲。
     * 4. 紫微斗数十天干四化(mutagen)
     *
     *    顺序为【禄，权，科，忌】
     *    - 甲:【廉，破，武，阳】
     *    - 乙:【机，梁，紫，月】
     *    - 丙:【同，机，昌，廉】
     *    - 丁:【阴，同，机，巨】
     *    - 戊:【贪，月，弼，机】
     *    - 己:【武，贪，梁，曲】
     *    - 庚:【阳，武，阴，同】
     *    - 辛:【巨，日，曲，昌】
     *    - 壬:【梁，紫，左，武】
     *    - 癸:【破，巨，阴，贪】
     */
    export const heavenlyStems: {
        readonly jiaHeavenly: {
            readonly yinYang: "阳";
            readonly fiveElements: "木";
            readonly crash: "gengHeavenly";
            readonly mutagen: readonly [
                "lianzhenMaj",
                "pojunMaj",
                "wuquMaj",
                "taiyangMaj"
            ];
        };
        readonly yiHeavenly: {
            readonly yinYang: "阴";
            readonly fiveElements: "木";
            readonly crash: "xinHeavenly";
            readonly mutagen: readonly [
                "tianjiMaj",
                "tianliangMaj",
                "ziweiMaj",
                "taiyinMaj"
            ];
        };
        readonly bingHeavenly: {
            readonly yinYang: "阳";
            readonly fiveElements: "火";
            readonly crash: "renHeavenly";
            readonly mutagen: readonly [
                "tiantongMaj",
                "tianjiMaj",
                "wenchangMin",
                "lianzhenMaj"
            ];
        };
        readonly dingHeavenly: {
            readonly yinYang: "阴";
            readonly fiveElements: "火";
            readonly crash: "guiHeavenly";
            readonly mutagen: readonly [
                "taiyinMaj",
                "tiantongMaj",
                "tianjiMaj",
                "jumenMaj"
            ];
        };
        readonly wuHeavenly: {
            readonly yinYang: "阳";
            readonly fiveElements: "土";
            readonly mutagen: readonly [
                "tanlangMaj",
                "taiyinMaj",
                "youbiMin",
                "tianjiMaj"
            ];
        };
        readonly jiHeavenly: {
            readonly yinYang: "阴";
            readonly fiveElements: "土";
            readonly mutagen: readonly [
                "wuquMaj",
                "tanlangMaj",
                "tianliangMaj",
                "wenquMin"
            ];
        };
        readonly gengHeavenly: {
            readonly yinYang: "阳";
            readonly fiveElements: "金";
            readonly crash: "jiaHeavenly";
            readonly mutagen: readonly [
                "taiyangMaj",
                "wuquMaj",
                "taiyinMaj",
                "tiantongMaj"
            ];
        };
        readonly xinHeavenly: {
            readonly yinYang: "阴";
            readonly fiveElements: "金";
            readonly crash: "yiHeavenly";
            readonly mutagen: readonly [
                "jumenMaj",
                "taiyangMaj",
                "wenquMin",
                "wenchangMin"
            ];
        };
        readonly renHeavenly: {
            readonly yinYang: "阳";
            readonly fiveElements: "水";
            readonly crash: "bingHeavenly";
            readonly mutagen: readonly [
                "tianliangMaj",
                "ziweiMaj",
                "zuofuMin",
                "wuquMaj"
            ];
        };
        readonly guiHeavenly: {
            readonly yinYang: "阴";
            readonly fiveElements: "水";
            readonly crash: "dingHeavenly";
            readonly mutagen: readonly [
                "pojunMaj",
                "jumenMaj",
                "taiyinMaj",
                "tanlangMaj"
            ];
        };
    };
}

declare module "iztro/lib/data/index" {
    export * from "iztro/lib/data/constants";
    export * from "iztro/lib/data/stars";
    export * from "iztro/lib/data/heavenlyStems";
    export * from "iztro/lib/data/earthlyBranches";
}

declare module "iztro/lib/data/stars" {
    /** 紫微斗数四化 */
    export const MUTAGEN: readonly [
        "sihuaLu",
        "sihuaQuan",
        "sihuaKe",
        "sihuaJi"
    ];
    /**
     * 星耀信息
     * 其中包含：
     * 1. 亮度（bright）, 按照宫位地支排序（从寅开始）
     * 2. 五行（fiveElements）
     * 3. 阴阳（yinYang）
     */
    export const STARS_INFO: {
        readonly ziweiMaj: {
            readonly brightness: readonly [
                "wang",
                "wang",
                "de",
                "wang",
                "miao",
                "miao",
                "wang",
                "wang",
                "de",
                "wang",
                "ping",
                "miao"
            ];
            readonly fiveElements: "土";
            readonly yinYang: "阴";
        };
        readonly tianjiMaj: {
            readonly brightness: readonly [
                "de",
                "wang",
                "li",
                "ping",
                "miao",
                "xian",
                "de",
                "wang",
                "li",
                "ping",
                "miao",
                "xian"
            ];
            readonly fiveElements: "木";
            readonly yinYang: "阴";
        };
        readonly taiyangMaj: {
            readonly brightness: readonly [
                "wang",
                "miao",
                "wang",
                "wang",
                "wang",
                "de",
                "de",
                "xian",
                "bu",
                "xian",
                "xian",
                "bu"
            ];
            readonly fiveElements: "";
            readonly yinYang: "";
        };
        readonly wuquMaj: {
            readonly brightness: readonly [
                "de",
                "li",
                "miao",
                "ping",
                "wang",
                "miao",
                "de",
                "li",
                "miao",
                "ping",
                "wang",
                "miao"
            ];
            readonly fiveElements: "金";
            readonly yinYang: "阴";
        };
        readonly tiantongMaj: {
            readonly brightness: readonly [
                "li",
                "ping",
                "ping",
                "miao",
                "xian",
                "bu",
                "wang",
                "ping",
                "ping",
                "miao",
                "wang",
                "bu"
            ];
            readonly fiveElements: "水";
            readonly yinYang: "阳";
        };
        readonly lianzhenMaj: {
            readonly brightness: readonly [
                "miao",
                "ping",
                "li",
                "xian",
                "ping",
                "li",
                "miao",
                "ping",
                "li",
                "xian",
                "ping",
                "li"
            ];
            readonly fiveElements: "火";
            readonly yinYang: "阴";
        };
        readonly tianfuMaj: {
            readonly brightness: readonly [
                "miao",
                "de",
                "miao",
                "de",
                "wang",
                "miao",
                "de",
                "wang",
                "miao",
                "de",
                "miao",
                "miao"
            ];
            readonly fiveElements: "土";
            readonly yinYang: "阳";
        };
        readonly taiyinMaj: {
            readonly brightness: readonly [
                "wang",
                "xian",
                "xian",
                "xian",
                "bu",
                "bu",
                "li",
                "bu",
                "wang",
                "miao",
                "miao",
                "miao"
            ];
            readonly fiveElements: "水";
            readonly yinYang: "阴";
        };
        readonly tanlangMaj: {
            readonly brightness: readonly [
                "ping",
                "li",
                "miao",
                "xian",
                "wang",
                "miao",
                "ping",
                "li",
                "miao",
                "xian",
                "wang",
                "miao"
            ];
            readonly fiveElements: "水";
            readonly yinYang: "";
        };
        readonly jumenMaj: {
            readonly brightness: readonly [
                "miao",
                "miao",
                "xian",
                "wang",
                "wang",
                "bu",
                "miao",
                "miao",
                "xian",
                "wang",
                "wang",
                "bu"
            ];
            readonly fiveElements: "土";
            readonly yinYang: "阴";
        };
        readonly tianxiangMaj: {
            readonly brightness: readonly [
                "miao",
                "xian",
                "de",
                "de",
                "miao",
                "de",
                "miao",
                "xian",
                "de",
                "de",
                "miao",
                "miao"
            ];
            readonly fiveElements: "水";
            readonly yinYang: "";
        };
        readonly tianliangMaj: {
            readonly brightness: readonly [
                "miao",
                "miao",
                "miao",
                "xian",
                "miao",
                "wang",
                "xian",
                "de",
                "miao",
                "xian",
                "miao",
                "wang"
            ];
            readonly fiveElements: "土";
            readonly yinYang: "";
        };
        readonly qishaMaj: {
            readonly brightness: readonly [
                "miao",
                "wang",
                "miao",
                "ping",
                "wang",
                "miao",
                "miao",
                "miao",
                "miao",
                "ping",
                "wang",
                "miao"
            ];
            readonly fiveElements: "";
            readonly yinYang: "";
        };
        readonly pojunMaj: {
            readonly brightness: readonly [
                "de",
                "xian",
                "wang",
                "ping",
                "miao",
                "wang",
                "de",
                "xian",
                "wang",
                "ping",
                "miao",
                "wang"
            ];
            readonly fiveElements: "水";
            readonly yinYang: "";
        };
        readonly wenchangMin: {
            readonly brightness: readonly [
                "xian",
                "li",
                "de",
                "miao",
                "xian",
                "li",
                "de",
                "miao",
                "xian",
                "li",
                "de",
                "miao"
            ];
        };
        readonly wenquMin: {
            readonly brightness: readonly [
                "ping",
                "wang",
                "de",
                "miao",
                "xian",
                "wang",
                "de",
                "miao",
                "xian",
                "wang",
                "de",
                "miao"
            ];
        };
        readonly huoxingMin: {
            readonly brightness: readonly [
                "miao",
                "li",
                "xian",
                "de",
                "miao",
                "li",
                "xian",
                "de",
                "miao",
                "li",
                "xian",
                "de"
            ];
        };
        readonly lingxingMin: {
            readonly brightness: readonly [
                "miao",
                "li",
                "xian",
                "de",
                "miao",
                "li",
                "xian",
                "de",
                "miao",
                "li",
                "xian",
                "de"
            ];
        };
        readonly qingyangMin: {
            readonly brightness: readonly [
                "",
                "xian",
                "miao",
                "",
                "xian",
                "miao",
                "",
                "xian",
                "miao",
                "",
                "xian",
                "miao"
            ];
        };
        readonly tuoluoMin: {
            readonly brightness: readonly [
                "xian",
                "",
                "miao",
                "xian",
                "",
                "miao",
                "xian",
                "",
                "miao",
                "xian",
                "",
                "miao"
            ];
        };
    };
}

declare module "iztro/lib/data/types/astro" {
    import { IFunctionalPalace } from "iztro/lib/astro/FunctionalPalace";
    import { Brightness, EarthlyBranchName, FiveElementsClassName, GenderName, HeavenlyStemName, PalaceName, StarName } from "iztro/lib/i18n/index";
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    import { HeavenlyStemAndEarthlyBranchDate, LunarDate } from "lunar-lite/lib/types";
    import { Language } from "iztro/lib/data/types/general";
    /**
     * 运限对象
     *
     * @property
     * - index 所在宫位的索引
     * - heavenlyStem 该运限天干
     * - palaceNames 该运限的十二宫
     * - mutagen 四化星
     * - stars 流耀
     */
    export type HoroscopeItem = {
        /** 所在宫位的索引 */
        index: number;
        /** 运限名称 */
        name: string;
        /** 该运限天干 */
        heavenlyStem: HeavenlyStemName;
        /** 该运限地支 */
        earthlyBranch: EarthlyBranchName;
        /** 该运限的十二宫 */
        palaceNames: PalaceName[];
        /** 四化星 */
        mutagen: StarName[];
        /** 流耀 */
        stars?: FunctionalStar[][];
    };
    /**
     * 大限
     *
     * @property
     * - range 大限起止年龄 [起始年龄, 截止年龄]
     * - heavenlyStem 大限天干
     * - earthlyBranch 大限地支
     */
    export type Decadal = {
        /** 大限起止年龄 [起始年龄, 截止年龄] */
        range: [
            number,
            number
        ];
        /** 大限天干 */
        heavenlyStem: HeavenlyStemName;
        /** 大限地支 */
        earthlyBranch: EarthlyBranchName;
    };
    /**
     * 运限
     *
     * @property
     * - lunarDate 农历日期
     * - solarDate 阳历日期
     * - decadal 大限
     * - age 小限
     * - yearly 流年
     * - monthly 流月
     * - daily 流日
     */
    export type Horoscope = {
        /** 农历日期 */
        lunarDate: string;
        /** 阳历日期 */
        solarDate: string;
        /** 大限 */
        decadal: HoroscopeItem;
        /**
         * 小限
         *
         * @property
         * - nominalAge 虚岁
         */
        age: HoroscopeItem & {
            /** 虚岁 */
            nominalAge: number;
        };
        /** 流年 */
        yearly: HoroscopeItem & {
            yearlyDecStar: {
                jiangqian12: StarName[];
                suiqian12: StarName[];
            };
        };
        /** 流月 */
        monthly: HoroscopeItem;
        /** 流日 */
        daily: HoroscopeItem;
        /** 流时 */
        hourly: HoroscopeItem;
    };
    /**
     * 星盘对象
     *
     * @property
     * - solarDate 阳历日期
     * - lunarDate 农历日期
     * - chineseDate 干支纪年日期
     * - time 时辰
     * - timeRange 时辰对应的时间段
     * - sign 星座
     * - zodiac 生肖
     * - earthlyBranchOfSoulPalace 命宫地支
     * - earthlyBranchOfBodyPalace 身宫地支
     * - soul 命主
     * - body 身主
     * - palaces 十二宫数据
     *
     * @function
     * - horoscope() 获取运限数据
     * - palace() 获取宫位数据
     */
    export type Astrolabe = {
        /** 性别 */
        gender: string;
        /** 阳历日期 */
        solarDate: string;
        /** 农历日期 */
        lunarDate: string;
        /** 干支纪年日期 */
        chineseDate: string;
        /**
         * 原始日期数据，用于今后内部方法使用
         *
         * @property
         * - lunar 农历日期对象
         * - chinese 干支纪年日期对象
         */
        rawDates: {
            /** 农历日期对象 */
            lunarDate: LunarDate;
            /** 干支纪年日期对象 */
            chineseDate: HeavenlyStemAndEarthlyBranchDate;
        };
        /** 时辰 */
        time: string;
        /** 时辰对应的时间段 */
        timeRange: string;
        /** 星座 */
        sign: string;
        /** 生肖 */
        zodiac: string;
        /** 命宫地支 */
        earthlyBranchOfSoulPalace: EarthlyBranchName;
        /** 身宫地支 */
        earthlyBranchOfBodyPalace: EarthlyBranchName;
        /** 命主 */
        soul: StarName;
        /** 身主 */
        body: StarName;
        /** 五行局 */
        fiveElementsClass: FiveElementsClassName;
        /** 十二宫数据 */
        palaces: IFunctionalPalace[];
        /** 版权 */
        copyright: string;
    };
    /**
     * 定义一个接口，表示插件函数的类型
     * */
    export type Plugin = () => void;
    export type ConfigMutagens = Partial<Record<HeavenlyStemName, StarName[]>>;
    export type ConfigBrightness = Partial<Record<StarName, Brightness[]>>;
    export type AstroType = 'heaven' | 'earth' | 'human';
    export type Config = {
        /** 四化配置 */
        mutagens?: ConfigMutagens;
        /** 星耀亮度配置 */
        brightness?: ConfigBrightness;
        /** 年分割点配置，normal为正月初一分界，exact为立春分割 */
        yearDivide?: 'normal' | 'exact';
        /** 运限分割点配置，normal为正月初一分界，exact为立春分割 */
        horoscopeDivide?: 'normal' | 'exact';
        /** 小限分割点配置，normal为以自然年分界，birthday为生日分界 */
        ageDivide?: 'normal' | 'birthday';
        /** 晚子时配置，current时晚子时算当日，forward时晚子时算来日 */
        dayDivide?: 'current' | 'forward';
        /** 安星方法，default为通行版本，zhongzhou为中州派版本 */
        algorithm?: 'default' | 'zhongzhou';
    };
    /**
     * 排盘参数，该对象用于获取紫微斗数星盘数据。
     */
    export type Option = {
        /** 日期类型。
         * - 阳历：'solar'
         * - 阴历：'lunar'
         */
        type: 'solar' | 'lunar';
        /** 阳历日期，格式为YYYY-MM-DD */
        dateStr: string;
        /** 时辰索引。0为早子时，1为丑时，以此类推，12为晚子时 */
        timeIndex: number;
        /** 性别。支持多语言输入，比如可以输入 `男` 或 `女`，也可以输入 `male` 或 `femal` */
        gender: GenderName;
        /** 是否为闰月。仅阴历类型是可用，当月没有闰月时不生效。 */
        isLeapMonth?: boolean;
        /** 是否修正闰月。当修正闰月时，以农历15日为界，15日（含）之前算当月，之后算下月。 */
        fixLeap?: boolean;
        /** 输出语言。支持 'en-US', 'ja-JP', 'ko-KR', 'zh-CN', 'zh-TW', 'vi-VN' */
        language?: Language;
        /** 配置项。 */
        config?: Config;
        /**
         * 星盘类型（中州派特有）。默认为天盘。
         * - 天盘：'heaven'
         * - 地盘：'earth'
         * - 人盘：'human'
         */
        astroType?: AstroType;
    };
    /**
     * 通用排盘参数，该参数类型主要用于需要传递完整生辰和五行局起始干支的场景。
     * 五行局起始干支主要用于中州派的地盘、人盘排法。
     */
    export type AstrolabeParam = {
        /** 阳历日期，格式为YYYY-MM-DD */
        solarDate: string;
        /** 时辰索引【0～12】，0为早子时，12为晚子时 */
        timeIndex: number;
        /** 是否调整农历闰月（若该月不是闰月则不会生效），调整后闰月十五日之后按下月算 */
        fixLeap?: boolean;
        /** 性别，不是每个方法都需要性别，所以将之设置为可选 */
        gender?: GenderName;
        /** 五行局起始干支，用于中州派的地盘、人盘排法，不传该参数即为天盘数据 */
        from?: {
            heavenlyStem: HeavenlyStemName;
            earthlyBranch: EarthlyBranchName;
        };
    };
}

declare module "iztro/lib/data/types/general" {
    import { CHINESE_TIME, LANGUAGES, TIME_RANGE } from "iztro/lib/data/constants";
    /** 支持的语言 */
    export type Language = (typeof LANGUAGES)[number];
    /** 阴阳 */
    export type YinYang = '阴' | '阳';
    /** 五行 */
    export type FiveElements = '木' | '金' | '水' | '火' | '土';
    /** 时辰，子时分早晚 */
    export type ChineseTime = (typeof CHINESE_TIME)[number];
    /** 时辰对应的时间段 */
    export type TimeRange = (typeof TIME_RANGE)[number];
    /** 范围：本命｜大限｜流年|流月|流日|流时 */
    export type Scope = 'origin' | 'decadal' | 'yearly' | 'monthly' | 'daily' | 'hourly';
    /** 星耀类型 */
    export type StarType = 'major' | 'soft' | 'tough' | 'adjective' | 'flower' | 'helper' | 'lucun' | 'tianma';
}

declare module "iztro/lib/data/types/index" {
    export * from "iztro/lib/data/types/general";
    export * from "iztro/lib/data/types/star";
    export * from "iztro/lib/data/types/astro";
    export * from "iztro/lib/data/types/palace";
}

declare module "iztro/lib/data/types/palace" {
    import { IFunctionalPalace } from "iztro/lib/astro/FunctionalPalace";
    import { EarthlyBranchName, HeavenlyStemName, PalaceName, StarName } from "iztro/lib/i18n/index";
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    import { Decadal } from "iztro/lib/data/types/astro";
    /**
     * 命宫、身宫对象
     *
     * @property
     * - soulIndex 命宫索引
     * - bodyIndex 身宫索引
     * - heavenlyStemOfSoul 命宫天干
     * - earthlyBranchOfSoul 命宫地支
     */
    export type SoulAndBody = {
        /** 命宫索引 */
        soulIndex: number;
        /** 身宫索引 */
        bodyIndex: number;
        /** 命宫天干 */
        heavenlyStemOfSoul: HeavenlyStemName;
        /** 命宫地支 */
        earthlyBranchOfSoul: EarthlyBranchName;
    };
    /**
     * 宫位对象
     *
     * @property
     * - name 宫位名称
     * - isBodyPalace 是否身宫
     * - isOriginalPalace 是否来因宫
     * - heavenlyStem 宫位天干
     * - earthlyBranch 宫位地支
     * - majorStars 主星
     * - minorStars 辅星
     * - adjectiveStars 杂耀
     * - changsheng12 长生12神之一
     * - boshi12 博士12神之一
     * - jiangqian12 将前12神之一
     * - suiqian12 岁前12神之一
     * - decadal 大限
     * - ages 小限
     */
    export type Palace = {
        /** 宫位索引 */
        index: number;
        /** 宫位名称 */
        name: PalaceName;
        /** 是否身宫 */
        isBodyPalace: boolean;
        /** 是否来因宫 */
        isOriginalPalace: boolean;
        /** 宫位天干 */
        heavenlyStem: HeavenlyStemName;
        /** 宫位地支 */
        earthlyBranch: EarthlyBranchName;
        /** 主星 */
        majorStars: FunctionalStar[];
        /** 辅星 */
        minorStars: FunctionalStar[];
        /** 杂耀 */
        adjectiveStars: FunctionalStar[];
        /** 长生12神 */
        changsheng12: StarName;
        /** 博士12神 */
        boshi12: StarName;
        /** 流年将前12神 */
        jiangqian12: StarName;
        /** 流年岁前12神 */
        suiqian12: StarName;
        /** 大限 */
        decadal: Decadal;
        /** 小限 */
        ages: number[];
    };
    /**
     * 三方四正宫位
     *
     * @property
     * - target 目标宫位
     * - career 三方位（官禄位）
     * - opposite 对宫
     * - wealth 三方位（财帛位）
     */
    export type SurroundedPalaces = {
        /** 本宫 */
        target: IFunctionalPalace;
        /** 对宫 */
        opposite: IFunctionalPalace;
        /** 财帛位 */
        wealth: IFunctionalPalace;
        /** 官禄位 */
        career: IFunctionalPalace;
    };
}

declare module "iztro/lib/data/types/star" {
    import { Brightness, Mutagen, StarName } from "iztro/lib/i18n/index";
    import { Scope, StarType } from "iztro/lib/data/types/general";
    /**
     * 紫微斗数星耀
     *
     * @property
     * - name 星耀名字
     * - type 星耀类型
     * - scope 作用范围
     * - brightness 星耀亮度
     * - mutagen 四化
     */
    export type Star = {
        /** 星耀名字 */
        name: StarName;
        /** 星耀类型（主星 | 吉星 | 煞星 | 杂耀 | 桃花星 | 解神 | 禄存 | 天马） */
        type: StarType;
        /** 作用范围（本命盘 | 大限盘 | 流年盘） */
        scope: Scope;
        /** 星耀亮度，若没有亮度数据则此字段为`空字符串`或者 `undefined` */
        brightness?: Brightness;
        /** 四化，若未产生四化则此字段为 `undefined` */
        mutagen?: Mutagen;
    };
}

declare module "iztro/lib/i18n/index" {
    import i18next from "i18next";
    import { Language } from "iztro/lib/data/types/index";
    /**
     * 设置国际化语言。
     * 支持的语言见 type.ts -> Language
     *
     * @param language 需要设置的语言【默认为zh-CN】
     */
    export const setLanguage: (language: Language) => void;
    /**
     * 输出国际化文本。
     *
     * @param str 待翻译的字符串
     * @returns 翻译后的字符串
     */
    export const t: <T>(str: string) => T;
    /**
     * kot(Key of Translation).
     *
     * 通过翻译文本反查Key的值，用于各种计算。
     * 若没有找到则会返回Value本身。
     *
     * @param value 翻译后的字符串
     * @returns 翻译文本的Key值
     */
    export const kot: <T>(value: string, k?: string) => T;
    export * from "iztro/lib/i18n/types/index";
    export default i18next;
}

declare module "iztro/lib/i18n/locales/en-US/brightness" {
    const _default: {
        readonly miao: "[+3]";
        readonly wang: "[+2]";
        readonly de: "[+1]";
        readonly li: "[0]";
        readonly ping: "[-1]";
        readonly bu: "[-2]";
        readonly xian: "[-3]";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "zi";
        readonly chouEarthly: "chou";
        readonly yinEarthly: "yin";
        readonly maoEarthly: "mao";
        readonly chenEarthly: "chen";
        readonly siEarthly: "si";
        readonly wuEarthly: "woo";
        readonly weiEarthly: "wei";
        readonly shenEarthly: "shen";
        readonly youEarthly: "you";
        readonly xuEarthly: "xu";
        readonly haiEarthly: "hai";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/fiveElementsClass" {
    const _default: {
        readonly water2nd: "water 2nd";
        readonly wood3rd: "wood 3rd";
        readonly metal4th: "metal 4th";
        readonly earth5th: "earth 5th";
        readonly fire6th: "fire 6th";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/gender" {
    const _default: {
        readonly male: "male";
        readonly female: "female";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "jia";
        readonly yiHeavenly: "yi";
        readonly bingHeavenly: "bing";
        readonly dingHeavenly: "ding";
        readonly wuHeavenly: "wu";
        readonly jiHeavenly: "ji";
        readonly gengHeavenly: "geng";
        readonly xinHeavenly: "xin";
        readonly renHeavenly: "ren";
        readonly guiHeavenly: "gui";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/mutagen" {
    const _default: {
        readonly sihuaLu: "A";
        readonly sihuaQuan: "B";
        readonly sihuaKe: "C";
        readonly sihuaJi: "D";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/palace" {
    const _default: {
        readonly soulPalace: "soul";
        readonly bodyPalace: "body";
        readonly siblingsPalace: "siblings";
        readonly spousePalace: "spouse";
        readonly childrenPalace: "children";
        readonly wealthPalace: "wealth";
        readonly healthPalace: "health";
        readonly surfacePalace: "surface";
        readonly friendsPalace: "friends";
        readonly careerPalace: "career";
        readonly propertyPalace: "property";
        readonly spiritPalace: "spirit";
        readonly parentsPalace: "parents";
        readonly originalPalace: "origin";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/en-US/star" {
    const _default: {
        readonly ziweiMaj: "emperor";
        readonly tianjiMaj: "advisor";
        readonly taiyangMaj: "sun";
        readonly wuquMaj: "general";
        readonly tiantongMaj: "fortunate";
        readonly lianzhenMaj: "judge";
        readonly tianfuMaj: "empress";
        readonly taiyinMaj: "moon";
        readonly tanlangMaj: "wolf";
        readonly jumenMaj: "advocator";
        readonly tianxiangMaj: "minister";
        readonly tianliangMaj: "sage";
        readonly qishaMaj: "marshal";
        readonly pojunMaj: "rebel";
        readonly zuofuMin: "officer";
        readonly youbiMin: "helper";
        readonly wenchangMin: "scholar";
        readonly wenquMin: "artist";
        readonly lucunMin: "money";
        readonly tianmaMin: "horse";
        readonly qingyangMin: "driven";
        readonly tuoluoMin: "tangled";
        readonly huoxingMin: "impulsive";
        readonly lingxingMin: "spark";
        readonly tiankuiMin: "assistant";
        readonly tianyueMin: "aide";
        readonly dikongMin: "ideologue";
        readonly dijieMin: "fickle";
        readonly jieshaAdj: "murder";
        readonly tiankong: "utopian";
        readonly tianxing: "serious";
        readonly tianyao: "social";
        readonly jieshen: "considery";
        readonly yinsha: "gloomy";
        readonly tianxi: "cheerful";
        readonly tianguan: "solemn";
        readonly tianfu: "lucky";
        readonly tianku: "upset";
        readonly tianxu: "frail";
        readonly longchi: "talented";
        readonly fengge: "refined";
        readonly hongluan: "attractive";
        readonly guchen: "alone";
        readonly guasu: "lonely";
        readonly feilian: "instigated";
        readonly posui: "broken";
        readonly taifu: "honorable";
        readonly fenggao: "awarded";
        readonly tianwu: "psychic";
        readonly tianyue: "sickly";
        readonly santai: "senior";
        readonly bazuo: "dignified";
        readonly engguang: "grateful";
        readonly tiangui: "noble";
        readonly tiancai: "gifted";
        readonly tianshou: "ageless";
        readonly jiekong: "interrupted";
        readonly xunzhong: "meditative";
        readonly xunkong: "fancied";
        readonly kongwang: "bottomless";
        readonly jielu: "intercepted";
        readonly yuede: "peaceful";
        readonly tianshang: "wounded";
        readonly tianshi: "heaven";
        readonly tianchu: "gourmet";
        readonly changsheng: "born";
        readonly muyu: "infancy";
        readonly guandai: "adolescence";
        readonly linguan: "adulthood";
        readonly diwang: "prime";
        readonly shuai: "weak";
        readonly bing: "sick";
        readonly si: "dead";
        readonly mu: "buried";
        readonly jue: "dissipated";
        readonly tai: "embryo";
        readonly yang: "molding";
        readonly boshi: "doctor";
        readonly lishi: "sumo";
        readonly qinglong: "dragon";
        readonly xiaohao: "consumer";
        readonly jiangjun: "general";
        readonly zhoushu: "book";
        readonly faylian: "gossip";
        readonly xishen: "happiness";
        readonly bingfu: "illness";
        readonly dahao: "wastrel";
        readonly suipo: "wastrel";
        readonly fubing: "ambush";
        readonly guanfu: "government";
        readonly suijian: "initial";
        readonly huiqi: "unlucky";
        readonly sangmen: "downcast";
        readonly guansuo: "tied";
        readonly gwanfu: "official";
        readonly longde: "virtuous";
        readonly baihu: "sinister";
        readonly tiande: "blessed";
        readonly diaoke: "sorrowing";
        readonly jiangxing: "capable";
        readonly panan: "admired";
        readonly suiyi: "varied";
        readonly xiishen: "listless";
        readonly huagai: "religious";
        readonly jiesha: "robbed";
        readonly zhaisha: "disastery";
        readonly tiansha: "condemned";
        readonly zhibei: "insidious";
        readonly xianchi: "passionate";
        readonly yuesha: "hapless";
        readonly wangshen: "perished";
        readonly yunkui: "assistant(D)";
        readonly yunyue: "aide(D)";
        readonly yunchang: "scholar(D)";
        readonly yunqu: "artist(D)";
        readonly yunluan: "attractive(D)";
        readonly yunxi: "cheerful(D)";
        readonly yunlu: "money(D)";
        readonly yunyang: "driven(D)";
        readonly yuntuo: "tangled(D)";
        readonly yunma: "horse(D)";
        readonly liukui: "assistant(Y)";
        readonly liuyue: "aide(Y)";
        readonly liuchang: "scholar(Y)";
        readonly liuqu: "artist(Y)";
        readonly liuluan: "attractive(Y)";
        readonly liuxi: "cheerful(Y)";
        readonly liulu: "money(Y)";
        readonly liuyang: "driven(Y)";
        readonly liutuo: "tangled(Y)";
        readonly liuma: "horse(Y)";
        readonly nianjie: "considery(Y)";
        readonly yuekui: "assistant(M)";
        readonly yueyue: "aide(M)";
        readonly yuechang: "scholar(M)";
        readonly yuequ: "artist(M)";
        readonly yueluan: "attractive(M)";
        readonly yuexi: "cheerful(M)";
        readonly yuelu: "money(M)";
        readonly yueyang: "driven(M)";
        readonly yuetuo: "tangled(M)";
        readonly yuema: "horse(M)";
        readonly rikui: "assistant(d)";
        readonly riyue: "aide(d)";
        readonly richang: "scholar(d)";
        readonly riqu: "artist(d)";
        readonly riluan: "attractive(d)";
        readonly rixi: "cheerful(d)";
        readonly rilu: "money(d)";
        readonly riyang: "driven(d)";
        readonly rituo: "tangled(d)";
        readonly rima: "horse(d)";
        readonly shikui: "assistant(H)";
        readonly shiyue: "aide(H)";
        readonly shichang: "scholar(H)";
        readonly shiqu: "artist(H)";
        readonly shiluan: "attractive(H)";
        readonly shixi: "cheerful(H)";
        readonly shilu: "money(H)";
        readonly shiyang: "driven(H)";
        readonly shituo: "tangled(H)";
        readonly shima: "horse(H)";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/brightness" {
    const _default: {
        readonly miao: "廟";
        readonly wang: "旺";
        readonly de: "得";
        readonly li: "利";
        readonly ping: "平";
        readonly bu: "不";
        readonly xian: "陷";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "子";
        readonly chouEarthly: "丑";
        readonly yinEarthly: "寅";
        readonly maoEarthly: "卯";
        readonly chenEarthly: "辰";
        readonly siEarthly: "巳";
        readonly wuEarthly: "午";
        readonly weiEarthly: "未";
        readonly shenEarthly: "申";
        readonly youEarthly: "酉";
        readonly xuEarthly: "戌";
        readonly haiEarthly: "亥";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/fiveElementsClass" {
    const _default: {
        readonly water2nd: "水の二局";
        readonly wood3rd: "木の三局";
        readonly metal4th: "金の四局";
        readonly earth5th: "土の五局";
        readonly fire6th: "火の六局";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/gender" {
    const _default: {
        readonly male: "男";
        readonly female: "女";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "甲";
        readonly yiHeavenly: "乙";
        readonly bingHeavenly: "丙";
        readonly dingHeavenly: "丁";
        readonly wuHeavenly: "戊";
        readonly jiHeavenly: "己";
        readonly gengHeavenly: "庚";
        readonly xinHeavenly: "辛";
        readonly renHeavenly: "壬";
        readonly guiHeavenly: "癸";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/mutagen" {
    const _default: {
        readonly sihuaLu: "祿";
        readonly sihuaQuan: "權";
        readonly sihuaKe: "科";
        readonly sihuaJi: "忌";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/palace" {
    const _default: {
        readonly soulPalace: "命宮";
        readonly bodyPalace: "身宮";
        readonly siblingsPalace: "兄弟";
        readonly spousePalace: "夫妻";
        readonly childrenPalace: "子女";
        readonly wealthPalace: "財帛";
        readonly healthPalace: "疾厄";
        readonly surfacePalace: "遷移";
        readonly friendsPalace: "僕役";
        readonly careerPalace: "官祿";
        readonly propertyPalace: "田宅";
        readonly spiritPalace: "福德";
        readonly parentsPalace: "父母";
        readonly originalPalace: "来因";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ja-JP/star" {
    const _default: {
        readonly ziweiMaj: "紫微";
        readonly tianjiMaj: "天機";
        readonly taiyangMaj: "太陽";
        readonly wuquMaj: "武曲";
        readonly tiantongMaj: "天同";
        readonly lianzhenMaj: "廉貞";
        readonly tianfuMaj: "天府";
        readonly taiyinMaj: "太陰";
        readonly tanlangMaj: "貪狼";
        readonly jumenMaj: "巨門";
        readonly tianxiangMaj: "天相";
        readonly tianliangMaj: "天梁";
        readonly qishaMaj: "七殺";
        readonly pojunMaj: "破軍";
        readonly zuofuMin: "左輔";
        readonly youbiMin: "右弼";
        readonly wenchangMin: "文昌";
        readonly wenquMin: "文曲";
        readonly lucunMin: "祿存";
        readonly tianmaMin: "天馬";
        readonly qingyangMin: "擎羊";
        readonly tuoluoMin: "陀羅";
        readonly huoxingMin: "火星";
        readonly lingxingMin: "鈴星";
        readonly tiankuiMin: "天魁";
        readonly tianyueMin: "天鉞";
        readonly dikongMin: "地空";
        readonly dijieMin: "地劫";
        readonly jieshaAdj: "劫殺";
        readonly tiankong: "天空";
        readonly tianxing: "天刑";
        readonly tianyao: "天姚";
        readonly jieshen: "解神";
        readonly yinsha: "陰煞";
        readonly tianxi: "天喜";
        readonly tianguan: "天官";
        readonly tianfu: "天福";
        readonly tianku: "天哭";
        readonly tianxu: "天虛";
        readonly longchi: "龍池";
        readonly fengge: "鳳閣";
        readonly hongluan: "紅鸞";
        readonly guchen: "孤辰";
        readonly guasu: "寡宿";
        readonly feilian: "蜚廉";
        readonly posui: "破碎";
        readonly taifu: "台輔";
        readonly fenggao: "封誥";
        readonly tianwu: "天巫";
        readonly tianyue: "天月";
        readonly santai: "三台";
        readonly bazuo: "八座";
        readonly engguang: "恩光";
        readonly tiangui: "天貴";
        readonly tiancai: "天才";
        readonly tianshou: "天壽";
        readonly jiekong: "截空";
        readonly xunzhong: "旬中";
        readonly xunkong: "旬空";
        readonly kongwang: "空亡";
        readonly jielu: "截路";
        readonly yuede: "月德";
        readonly tianshang: "天傷";
        readonly tianshi: "天使";
        readonly tianchu: "天廚";
        readonly changsheng: "長生";
        readonly muyu: "沐浴";
        readonly guandai: "冠帶";
        readonly linguan: "臨官";
        readonly diwang: "帝旺";
        readonly shuai: "衰";
        readonly bing: "病";
        readonly si: "死";
        readonly mu: "墓";
        readonly jue: "絕";
        readonly tai: "胎";
        readonly yang: "養";
        readonly boshi: "博士";
        readonly lishi: "力士";
        readonly qinglong: "青龍";
        readonly xiaohao: "小耗";
        readonly jiangjun: "將軍";
        readonly zhoushu: "奏書";
        readonly faylian: "飛廉";
        readonly xishen: "喜神";
        readonly bingfu: "病符";
        readonly dahao: "大耗";
        readonly suipo: "歲破";
        readonly fubing: "伏兵";
        readonly guanfu: "官府";
        readonly suijian: "歲建";
        readonly huiqi: "晦氣";
        readonly sangmen: "喪門";
        readonly guansuo: "貫索";
        readonly gwanfu: "官符";
        readonly longde: "龍德";
        readonly baihu: "白虎";
        readonly tiande: "天德";
        readonly diaoke: "弔客";
        readonly jiangxing: "將星";
        readonly panan: "攀鞍";
        readonly suiyi: "歲驛";
        readonly xiishen: "息神";
        readonly huagai: "華蓋";
        readonly jiesha: "劫煞";
        readonly zhaisha: "災煞";
        readonly tiansha: "天煞";
        readonly zhibei: "指背";
        readonly xianchi: "咸池";
        readonly yuesha: "月煞";
        readonly wangshen: "亡神";
        readonly yunkui: "限の魁";
        readonly yunyue: "限の钺";
        readonly yunchang: "限の昌";
        readonly yunqu: "限の曲";
        readonly yunluan: "限の鸾";
        readonly yunxi: "限の喜";
        readonly yunlu: "限の祿";
        readonly yunyang: "限の羊";
        readonly yuntuo: "限の陀";
        readonly yunma: "限の馬";
        readonly liukui: "年の魁";
        readonly liuyue: "年の钺";
        readonly liuchang: "年の昌";
        readonly liuqu: "年の曲";
        readonly liuluan: "年の鸾";
        readonly liuxi: "年の喜";
        readonly liulu: "年の祿";
        readonly liuyang: "年の羊";
        readonly liutuo: "年の陀";
        readonly liuma: "年の馬";
        readonly nianjie: "年の解";
        readonly yuekui: "月の魁";
        readonly yueyue: "月の钺";
        readonly yuechang: "月の昌";
        readonly yuequ: "月の曲";
        readonly yueluan: "月の鸾";
        readonly yuexi: "月の喜";
        readonly yuelu: "月の祿";
        readonly yueyang: "月の羊";
        readonly yuetuo: "月の陀";
        readonly yuema: "月の馬";
        readonly rikui: "日の魁";
        readonly riyue: "日の钺";
        readonly richang: "日の昌";
        readonly riqu: "日の曲";
        readonly riluan: "日の鸾";
        readonly rixi: "日の喜";
        readonly rilu: "日の祿";
        readonly riyang: "日の羊";
        readonly rituo: "日の陀";
        readonly rima: "日の馬";
        readonly shikui: "時の魁";
        readonly shiyue: "時の钺";
        readonly shichang: "時の昌";
        readonly shiqu: "時の曲";
        readonly shiluan: "時の鸾";
        readonly shixi: "時の喜";
        readonly shilu: "時の祿";
        readonly shiyang: "時の羊";
        readonly shituo: "時の陀";
        readonly shima: "時の馬";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/brightness" {
    const _default: {
        readonly miao: "[+3]";
        readonly wang: "[+2]";
        readonly de: "[+1]";
        readonly li: "[0]";
        readonly ping: "[-1]";
        readonly bu: "[-2]";
        readonly xian: "[-3]";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "자";
        readonly chouEarthly: "축";
        readonly yinEarthly: "인";
        readonly maoEarthly: "묘";
        readonly chenEarthly: "진";
        readonly siEarthly: "사";
        readonly wuEarthly: "오";
        readonly weiEarthly: "미";
        readonly shenEarthly: "신";
        readonly youEarthly: "유";
        readonly xuEarthly: "술";
        readonly haiEarthly: "해";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/fiveElementsClass" {
    const _default: {
        readonly water2nd: "수이국";
        readonly wood3rd: "목삼국";
        readonly metal4th: "금사국";
        readonly earth5th: "토오국";
        readonly fire6th: "화육국";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/gender" {
    const _default: {
        readonly male: "남성";
        readonly female: "여자";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "갑";
        readonly yiHeavenly: "을";
        readonly bingHeavenly: "병";
        readonly dingHeavenly: "정";
        readonly wuHeavenly: "무";
        readonly jiHeavenly: "기";
        readonly gengHeavenly: "경";
        readonly xinHeavenly: "신";
        readonly renHeavenly: "임";
        readonly guiHeavenly: "계";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/mutagen" {
    const _default: {
        readonly sihuaLu: "록";
        readonly sihuaQuan: "권";
        readonly sihuaKe: "과";
        readonly sihuaJi: "기";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/palace" {
    const _default: {
        readonly soulPalace: "명궁";
        readonly bodyPalace: "신궁";
        readonly siblingsPalace: "형제";
        readonly spousePalace: "부처";
        readonly childrenPalace: "자녀";
        readonly wealthPalace: "재백";
        readonly healthPalace: "질액";
        readonly surfacePalace: "천이";
        readonly friendsPalace: "노복";
        readonly careerPalace: "관록";
        readonly propertyPalace: "전택";
        readonly spiritPalace: "복덕";
        readonly parentsPalace: "부모";
        readonly originalPalace: "라인";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/ko-KR/star" {
    const _default: {
        readonly ziweiMaj: "자미";
        readonly tianjiMaj: "천기";
        readonly taiyangMaj: "태양";
        readonly wuquMaj: "무곡";
        readonly tiantongMaj: "천동";
        readonly lianzhenMaj: "염정";
        readonly tianfuMaj: "천부";
        readonly taiyinMaj: "태음";
        readonly tanlangMaj: "탐랑";
        readonly jumenMaj: "거문";
        readonly tianxiangMaj: "천상";
        readonly tianliangMaj: "천량";
        readonly qishaMaj: "칠살";
        readonly pojunMaj: "파군";
        readonly zuofuMin: "좌보";
        readonly youbiMin: "우필";
        readonly wenchangMin: "문창";
        readonly wenquMin: "문곡";
        readonly lucunMin: "록존";
        readonly tianmaMin: "천마";
        readonly qingyangMin: "경양";
        readonly tuoluoMin: "타라";
        readonly huoxingMin: "화성";
        readonly lingxingMin: "령성";
        readonly tiankuiMin: "천괴";
        readonly tianyueMin: "천월";
        readonly dikongMin: "지공";
        readonly dijieMin: "지겁";
        readonly jieshaAdj: "겁살";
        readonly tiankong: "천공";
        readonly tianxing: "천형";
        readonly tianyao: "천요";
        readonly jieshen: "해신";
        readonly yinsha: "음살";
        readonly tianxi: "천희";
        readonly tianguan: "천관";
        readonly tianfu: "천복";
        readonly tianku: "천곡";
        readonly tianxu: "천허";
        readonly longchi: "용지";
        readonly fengge: "봉각";
        readonly hongluan: "홍란";
        readonly guchen: "고진";
        readonly guasu: "과숙";
        readonly feilian: "비렴";
        readonly posui: "파쇄";
        readonly taifu: "태보";
        readonly fenggao: "봉고";
        readonly tianwu: "천무";
        readonly tianyue: "천월";
        readonly santai: "삼태";
        readonly bazuo: "팔좌";
        readonly engguang: "은광";
        readonly tiangui: "천귀";
        readonly tiancai: "천재";
        readonly tianshou: "천수";
        readonly jiekong: "절중";
        readonly xunzhong: "순중";
        readonly xunkong: "순공";
        readonly kongwang: "공망";
        readonly jielu: "절로";
        readonly yuede: "월덕";
        readonly tianshang: "천상";
        readonly tianshi: "천사";
        readonly tianchu: "천주";
        readonly changsheng: "장생";
        readonly muyu: "목욕";
        readonly guandai: "관대";
        readonly linguan: "임관";
        readonly diwang: "제왕";
        readonly shuai: "쇠";
        readonly bing: "병";
        readonly si: "사";
        readonly mu: "묘";
        readonly jue: "절";
        readonly tai: "태";
        readonly yang: "양";
        readonly boshi: "박사";
        readonly lishi: "역사";
        readonly qinglong: "청룡";
        readonly xiaohao: "소모";
        readonly jiangjun: "장군";
        readonly zhoushu: "주서";
        readonly faylian: "비렴";
        readonly xishen: "희신";
        readonly bingfu: "병부";
        readonly dahao: "대모";
        readonly suipo: "태파";
        readonly fubing: "복병";
        readonly guanfu: "관부";
        readonly suijian: "태세";
        readonly huiqi: "회기";
        readonly sangmen: "상문";
        readonly guansuo: "관색";
        readonly gwanfu: "관부";
        readonly longde: "용덕";
        readonly baihu: "백호";
        readonly tiande: "복덕";
        readonly diaoke: "조객";
        readonly jiangxing: "장성";
        readonly panan: "반안";
        readonly suiyi: "세역";
        readonly xiishen: "식신";
        readonly huagai: "화개";
        readonly jiesha: "겁살";
        readonly zhaisha: "재살";
        readonly tiansha: "천살";
        readonly zhibei: "지배";
        readonly xianchi: "함지";
        readonly yuesha: "월살";
        readonly wangshen: "망신";
        readonly yunkui: "천괴(십년)";
        readonly yunyue: "천월(십년)";
        readonly yunchang: "문창(십년)";
        readonly yunqu: "문곡(십년)";
        readonly yunluan: "홍란(십년)";
        readonly yunxi: "천희(십년)";
        readonly yunlu: "록존(십년)";
        readonly yunyang: "경양(십년)";
        readonly yuntuo: "타라(십년)";
        readonly yunma: "천마(십년)";
        readonly liukui: "천괴(년)";
        readonly liuyue: "천월(년)";
        readonly liuchang: "문창(년)";
        readonly liuqu: "문곡(년)";
        readonly liuluan: "홍란(년)";
        readonly liuxi: "천희(년)";
        readonly liulu: "록존(년)";
        readonly liuyang: "경양(년)";
        readonly liutuo: "타라(년)";
        readonly liuma: "천마(년)";
        readonly nianjie: "해신(년)";
        readonly yuekui: "천괴(월)";
        readonly yueyue: "천월(월)";
        readonly yuechang: "문창(월)";
        readonly yuequ: "문곡(월)";
        readonly yueluan: "홍란(월)";
        readonly yuexi: "천희(월)";
        readonly yuelu: "록존(월)";
        readonly yueyang: "경양(월)";
        readonly yuetuo: "타라(월)";
        readonly yuema: "천마(월)";
        readonly rikui: "천괴(일)";
        readonly riyue: "천월(일)";
        readonly richang: "문창(일)";
        readonly riqu: "문곡(일)";
        readonly riluan: "홍란(일)";
        readonly rixi: "천희(일)";
        readonly rilu: "록존(일)";
        readonly riyang: "경양(일)";
        readonly rituo: "타라(일)";
        readonly rima: "천마(일)";
        readonly shikui: "천괴(시)";
        readonly shiyue: "천월(시)";
        readonly shichang: "문창(시)";
        readonly shiqu: "문곡(시)";
        readonly shiluan: "홍란(시)";
        readonly shixi: "천희(시)";
        readonly shilu: "록존(시)";
        readonly shiyang: "경양(시)";
        readonly shituo: "타라(시)";
        readonly shima: "천마(시)";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/brightness" {
    const _default: {
        readonly miao: "Miếu";
        readonly wang: "Vượng";
        readonly de: "Đắc";
        readonly li: "Lợi";
        readonly ping: "Bình";
        readonly bu: "Bất";
        readonly xian: "Hạn";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "Tý";
        readonly chouEarthly: "Sửu";
        readonly yinEarthly: "Dần";
        readonly maoEarthly: "Mão";
        readonly chenEarthly: "Thìn";
        readonly siEarthly: "Tỵ";
        readonly wuEarthly: "Ngọ";
        readonly weiEarthly: "Mùi";
        readonly shenEarthly: "Thân";
        readonly youEarthly: "Dậu";
        readonly xuEarthly: "Tuất";
        readonly haiEarthly: "Hợi";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/fiveElementsClass" {
    const _default: {
        readonly water2nd: "Thủy Nhị Cục";
        readonly wood3rd: "Mộc Tam Cục";
        readonly metal4th: "Kim Tứ Cục";
        readonly earth5th: "Thổ Ngũ Cục";
        readonly fire6th: "Hỏa Lục Cục";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/gender" {
    const _default: {
        readonly male: "Nam";
        readonly female: "Nữ";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "Giáp";
        readonly yiHeavenly: "Ất";
        readonly bingHeavenly: "Bính";
        readonly dingHeavenly: "Đinh";
        readonly wuHeavenly: "Mậu";
        readonly jiHeavenly: "Kỷ";
        readonly gengHeavenly: "Canh";
        readonly xinHeavenly: "Tân";
        readonly renHeavenly: "Nhâm";
        readonly guiHeavenly: "Quý";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/mutagen" {
    const _default: {
        readonly sihuaLu: "Lộc";
        readonly sihuaQuan: "Quyền";
        readonly sihuaKe: "Khoa";
        readonly sihuaJi: "Kỵ";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/palace" {
    const _default: {
        readonly soulPalace: "Mệnh";
        readonly bodyPalace: "Thân";
        readonly siblingsPalace: "Huynh Đệ";
        readonly spousePalace: "Phu Thê";
        readonly childrenPalace: "Tử Nữ";
        readonly wealthPalace: "Tài Bạch";
        readonly healthPalace: "Tật Ách";
        readonly surfacePalace: "Thiên Di";
        readonly friendsPalace: "Nô Bộc";
        readonly careerPalace: "Quan Lộc";
        readonly propertyPalace: "Điền Trạch";
        readonly spiritPalace: "Phúc Đức";
        readonly parentsPalace: "Phụ Mẫu";
        readonly originalPalace: "Lai Nhân";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/vi-VN/star" {
    const _default: {
        readonly ziweiMaj: "Tử Vi";
        readonly tianjiMaj: "Thiên Cơ";
        readonly taiyangMaj: "Thái Dương";
        readonly wuquMaj: "Vũ Khúc";
        readonly tiantongMaj: "Thiên Đồng";
        readonly lianzhenMaj: "Liêm Trinh";
        readonly tianfuMaj: "Thiên Phủ";
        readonly taiyinMaj: "Thái Âm";
        readonly tanlangMaj: "Tham Lang";
        readonly jumenMaj: "Cự Môn";
        readonly tianxiangMaj: "Thiên Tướng";
        readonly tianliangMaj: "Thiên Lương";
        readonly qishaMaj: "Thất Sát";
        readonly pojunMaj: "Phá Quân";
        readonly zuofuMin: "Tả Phù";
        readonly youbiMin: "Hữu Bật";
        readonly wenchangMin: "Văn Xương";
        readonly wenquMin: "Văn Khúc";
        readonly lucunMin: "Lộc Tồn";
        readonly tianmaMin: "Thiên Mã";
        readonly qingyangMin: "Kình Dương";
        readonly tuoluoMin: "Đà La";
        readonly huoxingMin: "Hỏa Tinh";
        readonly lingxingMin: "Linh Tinh";
        readonly tiankuiMin: "Thiên Khôi";
        readonly tianyueMin: "Thiên Việt";
        readonly dikongMin: "Địa Không";
        readonly dijieMin: "Địa Kiếp";
        readonly jieshaAdj: "Kiếp Sát";
        readonly tiankong: "Thiên Không";
        readonly tianxing: "Thiên Hình";
        readonly tianyao: "Thiên Diêu";
        readonly jieshen: "Giải Thần";
        readonly yinsha: "Âm Sát";
        readonly tianxi: "Thiên Hỷ";
        readonly tianguan: "Thiên Quan";
        readonly tianfu: "Thiên Phúc";
        readonly tianku: "Thiên Khốc";
        readonly tianxu: "Thiên Hư";
        readonly longchi: "Long Trì";
        readonly fengge: "Phụng Các";
        readonly hongluan: "Hồng Loan";
        readonly guchen: "Cô Thần";
        readonly guasu: "Quả Tú";
        readonly feilian: "Phi Liêm";
        readonly posui: "Phá Toái";
        readonly taifu: "Đài Phụ";
        readonly fenggao: "Phong Cáo";
        readonly tianwu: "Thiên Vu";
        readonly tianyue: "Thiên Nguyệt";
        readonly santai: "Tam Thai";
        readonly bazuo: "Bát Tọa";
        readonly engguang: "Ân Quang";
        readonly tiangui: "Thiên Quý";
        readonly tiancai: "Thiên Tài";
        readonly tianshou: "Thiên Thọ";
        readonly jiekong: "Triệt Không";
        readonly xunzhong: "Tuần Trung";
        readonly xunkong: "Tuần Không";
        readonly kongwang: "Không Vong";
        readonly jielu: "Triệt Lộ";
        readonly yuede: "Nguyệt Đức";
        readonly tianshang: "Thiên Thương";
        readonly tianshi: "Thiên Sứ";
        readonly tianchu: "Thiên Trù";
        readonly changsheng: "Trường Sinh";
        readonly muyu: "Mục Dục";
        readonly guandai: "Quan Đới";
        readonly linguan: "Lâm Quan";
        readonly diwang: "Đế Vượng";
        readonly shuai: "Suy";
        readonly bing: "Bệnh";
        readonly si: "Tử";
        readonly mu: "Mộ";
        readonly jue: "Tuyệt";
        readonly tai: "Thai";
        readonly yang: "Dưỡng";
        readonly boshi: "Bác Sỹ";
        readonly lishi: "Lực Sỹ";
        readonly qinglong: "Thanh Long";
        readonly xiaohao: "Tiểu Hao";
        readonly jiangjun: "Tướng Quân";
        readonly zhoushu: "Tấu Thư";
        readonly faylian: "Phi Liêm";
        readonly xishen: "Hỷ Thần";
        readonly bingfu: "Bệnh Phù";
        readonly dahao: "Đại Hao";
        readonly suipo: "Tuế Phá";
        readonly fubing: "Phục Binh";
        readonly guanfu: "Quan Phủ";
        readonly suijian: "Tuế Kiện";
        readonly huiqi: "Hối Khí";
        readonly sangmen: "Tang Môn";
        readonly guansuo: "Quán Tác";
        readonly gwanfu: "Quan Phù";
        readonly longde: "Long Đức";
        readonly baihu: "Bạch Hổ";
        readonly tiande: "Thiên Đức";
        readonly diaoke: "Điếu Khách";
        readonly jiangxing: "Tướng Tinh";
        readonly panan: "Phan Án";
        readonly suiyi: "Tuế Dịch";
        readonly xiishen: "Tức Thần";
        readonly huagai: "Hoa Cái";
        readonly jiesha: "Kiếp Sát";
        readonly zhaisha: "Tai Sát";
        readonly tiansha: "Thiên Sát";
        readonly zhibei: "Chỉ Bối";
        readonly xianchi: "Hàm Trì";
        readonly yuesha: "Nguyệt Sát";
        readonly wangshen: "Vong Thần";
        readonly yunkui: "Vận Khôi";
        readonly yunyue: "Vận Việt";
        readonly yunchang: "Vận Xương";
        readonly yunqu: "Vận Khúc";
        readonly yunluan: "Vận Loan";
        readonly yunxi: "Vận Hỷ";
        readonly yunlu: "Vận Lộc";
        readonly yunyang: "Vận Dương";
        readonly yuntuo: "Vận Đà";
        readonly yunma: "Vận Mã";
        readonly liukui: "Lưu Khôi";
        readonly liuyue: "Lưu Việt";
        readonly liuchang: "Lưu Xương";
        readonly liuqu: "Lưu Khúc";
        readonly liuluan: "Lưu Loan";
        readonly liuxi: "Lưu Hỷ";
        readonly liulu: "Lưu Lộc";
        readonly liuyang: "Lưu Dương";
        readonly liutuo: "Lưu Đà";
        readonly liuma: "Lưu Mã";
        readonly nianjie: "Niên Giải";
        readonly yuekui: "Thiên Khôi(M)";
        readonly yueyue: "Thiên Nguyệt(M)";
        readonly yuechang: "Văn Xương(M)";
        readonly yuequ: "Văn Khúc(M)";
        readonly yueluan: "Hồng Loan(M)";
        readonly yuexi: "Thiên Hỷ(M)";
        readonly yuelu: "Lộc Tồn(M)";
        readonly yueyang: "Kình Dương(M)";
        readonly yuetuo: "Đà La(M)";
        readonly yuema: "Thiên Mã(M)";
        readonly rikui: "Thiên Khôi(d)";
        readonly riyue: "Thiên Nguyệt(d)";
        readonly richang: "Văn Xương(d)";
        readonly riqu: "Văn Khúc(d)";
        readonly riluan: "Hồng Loan(d)";
        readonly rixi: "Thiên Hỷ(d)";
        readonly rilu: "Lộc Tồn(d)";
        readonly riyang: "Kình Dương(d)";
        readonly rituo: "Đà La(d)";
        readonly rima: "Thiên Mã(d)";
        readonly shikui: "Thiên Khôi(H)";
        readonly shiyue: "Thiên Nguyệt(H)";
        readonly shichang: "Văn Xương(H)";
        readonly shiqu: "Văn Khúc(H)";
        readonly shiluan: "Hồng Loan(H)";
        readonly shixi: "Thiên Hỷ(H)";
        readonly shilu: "Lộc Tồn(H)";
        readonly shiyang: "Kình Dương(H)";
        readonly shituo: "Đà La(H)";
        readonly shima: "Thiên Mã(H)";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/brightness" {
    const _default: {
        readonly miao: "庙";
        readonly wang: "旺";
        readonly de: "得";
        readonly li: "利";
        readonly ping: "平";
        readonly bu: "不";
        readonly xian: "陷";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "子";
        readonly chouEarthly: "丑";
        readonly yinEarthly: "寅";
        readonly maoEarthly: "卯";
        readonly chenEarthly: "辰";
        readonly siEarthly: "巳";
        readonly wuEarthly: "午";
        readonly weiEarthly: "未";
        readonly shenEarthly: "申";
        readonly youEarthly: "酉";
        readonly xuEarthly: "戌";
        readonly haiEarthly: "亥";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/fiveElementsClass" {
    const _default: {
        readonly water2nd: "水二局";
        readonly wood3rd: "木三局";
        readonly metal4th: "金四局";
        readonly earth5th: "土五局";
        readonly fire6th: "火六局";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/gender" {
    const _default: {
        readonly male: "男";
        readonly female: "女";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "甲";
        readonly yiHeavenly: "乙";
        readonly bingHeavenly: "丙";
        readonly dingHeavenly: "丁";
        readonly wuHeavenly: "戊";
        readonly jiHeavenly: "己";
        readonly gengHeavenly: "庚";
        readonly xinHeavenly: "辛";
        readonly renHeavenly: "壬";
        readonly guiHeavenly: "癸";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/mutagen" {
    const _default: {
        readonly sihuaLu: "禄";
        readonly sihuaQuan: "权";
        readonly sihuaKe: "科";
        readonly sihuaJi: "忌";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/palace" {
    const _default: {
        readonly soulPalace: "命宫";
        readonly bodyPalace: "身宫";
        readonly siblingsPalace: "兄弟";
        readonly spousePalace: "夫妻";
        readonly childrenPalace: "子女";
        readonly wealthPalace: "财帛";
        readonly healthPalace: "疾厄";
        readonly surfacePalace: "迁移";
        readonly friendsPalace: "仆役";
        readonly careerPalace: "官禄";
        readonly propertyPalace: "田宅";
        readonly spiritPalace: "福德";
        readonly parentsPalace: "父母";
        readonly originalPalace: "来因";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-CN/star" {
    const _default: {
        readonly ziweiMaj: "紫微";
        readonly tianjiMaj: "天机";
        readonly taiyangMaj: "太阳";
        readonly wuquMaj: "武曲";
        readonly tiantongMaj: "天同";
        readonly lianzhenMaj: "廉贞";
        readonly tianfuMaj: "天府";
        readonly taiyinMaj: "太阴";
        readonly tanlangMaj: "贪狼";
        readonly jumenMaj: "巨门";
        readonly tianxiangMaj: "天相";
        readonly tianliangMaj: "天梁";
        readonly qishaMaj: "七杀";
        readonly pojunMaj: "破军";
        readonly zuofuMin: "左辅";
        readonly youbiMin: "右弼";
        readonly wenchangMin: "文昌";
        readonly wenquMin: "文曲";
        readonly lucunMin: "禄存";
        readonly tianmaMin: "天马";
        readonly qingyangMin: "擎羊";
        readonly tuoluoMin: "陀罗";
        readonly huoxingMin: "火星";
        readonly lingxingMin: "铃星";
        readonly tiankuiMin: "天魁";
        readonly tianyueMin: "天钺";
        readonly dikongMin: "地空";
        readonly dijieMin: "地劫";
        readonly jieshaAdj: "劫杀";
        readonly tiankong: "天空";
        readonly tianxing: "天刑";
        readonly tianyao: "天姚";
        readonly jieshen: "解神";
        readonly yinsha: "阴煞";
        readonly tianxi: "天喜";
        readonly tianguan: "天官";
        readonly tianfu: "天福";
        readonly tianku: "天哭";
        readonly tianxu: "天虚";
        readonly longchi: "龙池";
        readonly fengge: "凤阁";
        readonly hongluan: "红鸾";
        readonly guchen: "孤辰";
        readonly guasu: "寡宿";
        readonly feilian: "蜚廉";
        readonly posui: "破碎";
        readonly taifu: "台辅";
        readonly fenggao: "封诰";
        readonly tianwu: "天巫";
        readonly tianyue: "天月";
        readonly santai: "三台";
        readonly bazuo: "八座";
        readonly engguang: "恩光";
        readonly tiangui: "天贵";
        readonly tiancai: "天才";
        readonly tianshou: "天寿";
        readonly jiekong: "截空";
        readonly xunzhong: "旬中";
        readonly xunkong: "旬空";
        readonly kongwang: "空亡";
        readonly jielu: "截路";
        readonly yuede: "月德";
        readonly tianshang: "天伤";
        readonly tianshi: "天使";
        readonly tianchu: "天厨";
        readonly changsheng: "长生";
        readonly muyu: "沐浴";
        readonly guandai: "冠带";
        readonly linguan: "临官";
        readonly diwang: "帝旺";
        readonly shuai: "衰";
        readonly bing: "病";
        readonly si: "死";
        readonly mu: "墓";
        readonly jue: "绝";
        readonly tai: "胎";
        readonly yang: "养";
        readonly boshi: "博士";
        readonly lishi: "力士";
        readonly qinglong: "青龙";
        readonly xiaohao: "小耗";
        readonly jiangjun: "将军";
        readonly zhoushu: "奏书";
        readonly faylian: "飞廉";
        readonly xishen: "喜神";
        readonly bingfu: "病符";
        readonly dahao: "大耗";
        readonly suipo: "岁破";
        readonly fubing: "伏兵";
        readonly guanfu: "官府";
        readonly suijian: "岁建";
        readonly huiqi: "晦气";
        readonly sangmen: "丧门";
        readonly guansuo: "贯索";
        readonly gwanfu: "官符";
        readonly longde: "龙德";
        readonly baihu: "白虎";
        readonly tiande: "天德";
        readonly diaoke: "吊客";
        readonly jiangxing: "将星";
        readonly panan: "攀鞍";
        readonly suiyi: "岁驿";
        readonly xiishen: "息神";
        readonly huagai: "华盖";
        readonly jiesha: "劫煞";
        readonly zhaisha: "灾煞";
        readonly tiansha: "天煞";
        readonly zhibei: "指背";
        readonly xianchi: "咸池";
        readonly yuesha: "月煞";
        readonly wangshen: "亡神";
        readonly yunkui: "运魁";
        readonly yunyue: "运钺";
        readonly yunchang: "运昌";
        readonly yunqu: "运曲";
        readonly yunluan: "运鸾";
        readonly yunxi: "运喜";
        readonly yunlu: "运禄";
        readonly yunyang: "运羊";
        readonly yuntuo: "运陀";
        readonly yunma: "运马";
        readonly liukui: "流魁";
        readonly liuyue: "流钺";
        readonly liuchang: "流昌";
        readonly liuqu: "流曲";
        readonly liuluan: "流鸾";
        readonly liuxi: "流喜";
        readonly liulu: "流禄";
        readonly liuyang: "流羊";
        readonly liutuo: "流陀";
        readonly liuma: "流马";
        readonly nianjie: "年解";
        readonly yuekui: "月魁";
        readonly yueyue: "月钺";
        readonly yuechang: "月昌";
        readonly yuequ: "月曲";
        readonly yueluan: "月鸾";
        readonly yuexi: "月喜";
        readonly yuelu: "月禄";
        readonly yueyang: "月羊";
        readonly yuetuo: "月陀";
        readonly yuema: "月马";
        readonly rikui: "日魁";
        readonly riyue: "日钺";
        readonly richang: "日昌";
        readonly riqu: "日曲";
        readonly riluan: "日鸾";
        readonly rixi: "日喜";
        readonly rilu: "日禄";
        readonly riyang: "日羊";
        readonly rituo: "日陀";
        readonly rima: "日马";
        readonly shikui: "时魁";
        readonly shiyue: "时钺";
        readonly shichang: "时昌";
        readonly shiqu: "时曲";
        readonly shiluan: "时鸾";
        readonly shixi: "时喜";
        readonly shilu: "时禄";
        readonly shiyang: "时羊";
        readonly shituo: "时陀";
        readonly shima: "时马";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/brightness" {
    const _default: {
        readonly miao: "廟";
        readonly wang: "旺";
        readonly de: "得";
        readonly li: "利";
        readonly ping: "平";
        readonly bu: "不";
        readonly xian: "陷";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/earthlyBranch" {
    const _default: {
        readonly ziEarthly: "子";
        readonly chouEarthly: "丑";
        readonly yinEarthly: "寅";
        readonly maoEarthly: "卯";
        readonly chenEarthly: "辰";
        readonly siEarthly: "巳";
        readonly wuEarthly: "午";
        readonly weiEarthly: "未";
        readonly shenEarthly: "申";
        readonly youEarthly: "酉";
        readonly xuEarthly: "戌";
        readonly haiEarthly: "亥";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/fiveElementsClass" {
    const _default: {
        readonly water2nd: "水二局";
        readonly wood3rd: "木三局";
        readonly metal4th: "金四局";
        readonly earth5th: "土五局";
        readonly fire6th: "火六局";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/gender" {
    const _default: {
        readonly male: "男";
        readonly female: "女";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/heavenlyStem" {
    const _default: {
        readonly jiaHeavenly: "甲";
        readonly yiHeavenly: "乙";
        readonly bingHeavenly: "丙";
        readonly dingHeavenly: "丁";
        readonly wuHeavenly: "戊";
        readonly jiHeavenly: "己";
        readonly gengHeavenly: "庚";
        readonly xinHeavenly: "辛";
        readonly renHeavenly: "壬";
        readonly guiHeavenly: "癸";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/mutagen" {
    const _default: {
        readonly sihuaLu: "祿";
        readonly sihuaQuan: "權";
        readonly sihuaKe: "科";
        readonly sihuaJi: "忌";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/palace" {
    const _default: {
        readonly soulPalace: "命宮";
        readonly bodyPalace: "身宮";
        readonly siblingsPalace: "兄弟";
        readonly spousePalace: "夫妻";
        readonly childrenPalace: "子女";
        readonly wealthPalace: "財帛";
        readonly healthPalace: "疾厄";
        readonly surfacePalace: "遷移";
        readonly friendsPalace: "僕役";
        readonly careerPalace: "官祿";
        readonly propertyPalace: "田宅";
        readonly spiritPalace: "福德";
        readonly parentsPalace: "父母";
        readonly originalPalace: "来因";
    };
    export default _default;
}

declare module "iztro/lib/i18n/locales/zh-TW/star" {
    const _default: {
        readonly ziweiMaj: "紫微";
        readonly tianjiMaj: "天機";
        readonly taiyangMaj: "太陽";
        readonly wuquMaj: "武曲";
        readonly tiantongMaj: "天同";
        readonly lianzhenMaj: "廉貞";
        readonly tianfuMaj: "天府";
        readonly taiyinMaj: "太陰";
        readonly tanlangMaj: "貪狼";
        readonly jumenMaj: "巨門";
        readonly tianxiangMaj: "天相";
        readonly tianliangMaj: "天梁";
        readonly qishaMaj: "七殺";
        readonly pojunMaj: "破軍";
        readonly zuofuMin: "左輔";
        readonly youbiMin: "右弼";
        readonly wenchangMin: "文昌";
        readonly wenquMin: "文曲";
        readonly lucunMin: "祿存";
        readonly tianmaMin: "天馬";
        readonly qingyangMin: "擎羊";
        readonly tuoluoMin: "陀羅";
        readonly huoxingMin: "火星";
        readonly lingxingMin: "鈴星";
        readonly tiankuiMin: "天魁";
        readonly tianyueMin: "天鉞";
        readonly dikongMin: "地空";
        readonly dijieMin: "地劫";
        readonly jieshaAdj: "劫殺";
        readonly tiankong: "天空";
        readonly tianxing: "天刑";
        readonly tianyao: "天姚";
        readonly jieshen: "解神";
        readonly yinsha: "陰煞";
        readonly tianxi: "天喜";
        readonly tianguan: "天官";
        readonly tianfu: "天福";
        readonly tianku: "天哭";
        readonly tianxu: "天虛";
        readonly longchi: "龍池";
        readonly fengge: "鳳閣";
        readonly hongluan: "紅鸞";
        readonly guchen: "孤辰";
        readonly guasu: "寡宿";
        readonly feilian: "蜚廉";
        readonly posui: "破碎";
        readonly taifu: "台輔";
        readonly fenggao: "封誥";
        readonly tianwu: "天巫";
        readonly tianyue: "天月";
        readonly santai: "三台";
        readonly bazuo: "八座";
        readonly engguang: "恩光";
        readonly tiangui: "天貴";
        readonly tiancai: "天才";
        readonly tianshou: "天壽";
        readonly jiekong: "截空";
        readonly xunzhong: "旬中";
        readonly xunkong: "旬空";
        readonly kongwang: "空亡";
        readonly jielu: "截路";
        readonly yuede: "月德";
        readonly tianshang: "天傷";
        readonly tianshi: "天使";
        readonly tianchu: "天廚";
        readonly changsheng: "長生";
        readonly muyu: "沐浴";
        readonly guandai: "冠帶";
        readonly linguan: "臨官";
        readonly diwang: "帝旺";
        readonly shuai: "衰";
        readonly bing: "病";
        readonly si: "死";
        readonly mu: "墓";
        readonly jue: "絕";
        readonly tai: "胎";
        readonly yang: "養";
        readonly boshi: "博士";
        readonly lishi: "力士";
        readonly qinglong: "青龍";
        readonly xiaohao: "小耗";
        readonly jiangjun: "將軍";
        readonly zhoushu: "奏書";
        readonly faylian: "飛廉";
        readonly xishen: "喜神";
        readonly bingfu: "病符";
        readonly dahao: "大耗";
        readonly suipo: "歲破";
        readonly fubing: "伏兵";
        readonly guanfu: "官府";
        readonly suijian: "歲建";
        readonly huiqi: "晦氣";
        readonly sangmen: "喪門";
        readonly guansuo: "貫索";
        readonly gwanfu: "官符";
        readonly longde: "龍德";
        readonly baihu: "白虎";
        readonly tiande: "天德";
        readonly diaoke: "弔客";
        readonly jiangxing: "將星";
        readonly panan: "攀鞍";
        readonly suiyi: "歲驛";
        readonly xiishen: "息神";
        readonly huagai: "華蓋";
        readonly jiesha: "劫煞";
        readonly zhaisha: "災煞";
        readonly tiansha: "天煞";
        readonly zhibei: "指背";
        readonly xianchi: "咸池";
        readonly yuesha: "月煞";
        readonly wangshen: "亡神";
        readonly yunkui: "運魁";
        readonly yunyue: "運鉞";
        readonly yunchang: "運昌";
        readonly yunqu: "運曲";
        readonly yunluan: "運鸞";
        readonly yunxi: "運喜";
        readonly yunlu: "運祿";
        readonly yunyang: "運羊";
        readonly yuntuo: "運陀";
        readonly yunma: "運馬";
        readonly liukui: "流魁";
        readonly liuyue: "流鉞";
        readonly liuchang: "流昌";
        readonly liuqu: "流曲";
        readonly liuluan: "流鸞";
        readonly liuxi: "流喜";
        readonly liulu: "流祿";
        readonly liuyang: "流羊";
        readonly liutuo: "流陀";
        readonly liuma: "流馬";
        readonly nianjie: "年解";
        readonly yuekui: "月魁";
        readonly yueyue: "月鉞";
        readonly yuechang: "月昌";
        readonly yuequ: "月曲";
        readonly yueluan: "月鸞";
        readonly yuexi: "月喜";
        readonly yuelu: "月祿";
        readonly yueyang: "月羊";
        readonly yuetuo: "月陀";
        readonly yuema: "月馬";
        readonly rikui: "日魁";
        readonly riyue: "日鉞";
        readonly richang: "日昌";
        readonly riqu: "日曲";
        readonly riluan: "日鸞";
        readonly rixi: "日喜";
        readonly rilu: "日祿";
        readonly riyang: "日羊";
        readonly rituo: "日陀";
        readonly rima: "日馬";
        readonly shikui: "時魁";
        readonly shiyue: "時鉞";
        readonly shichang: "時昌";
        readonly shiqu: "時曲";
        readonly shiluan: "時鸞";
        readonly shixi: "時喜";
        readonly shilu: "時祿";
        readonly shiyang: "時羊";
        readonly shituo: "時陀";
        readonly shima: "時馬";
    };
    export default _default;
}

declare module "iztro/lib/i18n/types/Brightness" {
    import brightnessEnUS from "iztro/lib/i18n/locales/en-US/brightness";
    import brightnessJaJP from "iztro/lib/i18n/locales/ja-JP/brightness";
    import brightnessKoKR from "iztro/lib/i18n/locales/ko-KR/brightness";
    import brightnessZhCN from "iztro/lib/i18n/locales/zh-CN/brightness";
    import brightnessZhTW from "iztro/lib/i18n/locales/zh-TW/brightness";
    import brightnessViVN from "iztro/lib/i18n/locales/vi-VN/brightness";
    type BrightnessEnUS = (typeof brightnessEnUS)[keyof typeof brightnessEnUS];
    type BrightnessJaJP = (typeof brightnessJaJP)[keyof typeof brightnessJaJP];
    type BrightnessKoKR = (typeof brightnessKoKR)[keyof typeof brightnessKoKR];
    type BrightnessZhCN = (typeof brightnessZhCN)[keyof typeof brightnessZhCN];
    type BrightnessZhTW = (typeof brightnessZhTW)[keyof typeof brightnessZhTW];
    type BrightnessViVN = (typeof brightnessViVN)[keyof typeof brightnessViVN];
    export type Brightness = '' | BrightnessEnUS | BrightnessJaJP | BrightnessKoKR | BrightnessZhCN | BrightnessZhTW | BrightnessViVN;
    export type BrightnessKey = keyof typeof brightnessZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/EarthlyBranch" {
    import earthlyBranchEnUS from "iztro/lib/i18n/locales/en-US/earthlyBranch";
    import earthlyBranchJaJP from "iztro/lib/i18n/locales/ja-JP/earthlyBranch";
    import earthlyBranchKoKR from "iztro/lib/i18n/locales/ko-KR/earthlyBranch";
    import earthlyBranchZhCN from "iztro/lib/i18n/locales/zh-CN/earthlyBranch";
    import earthlyBranchZhTW from "iztro/lib/i18n/locales/zh-TW/earthlyBranch";
    import earthlyBranchViVN from "iztro/lib/i18n/locales/vi-VN/earthlyBranch";
    type EarthlyBranchEnUS = (typeof earthlyBranchEnUS)[keyof typeof earthlyBranchEnUS];
    type EarthlyBranchJaJP = (typeof earthlyBranchJaJP)[keyof typeof earthlyBranchJaJP];
    type EarthlyBranchKoKR = (typeof earthlyBranchKoKR)[keyof typeof earthlyBranchKoKR];
    type EarthlyBranchZhCN = (typeof earthlyBranchZhCN)[keyof typeof earthlyBranchZhCN];
    type EarthlyBranchZhTW = (typeof earthlyBranchZhTW)[keyof typeof earthlyBranchZhTW];
    type EarthlyBranchViVN = (typeof earthlyBranchViVN)[keyof typeof earthlyBranchViVN];
    export type EarthlyBranchName = EarthlyBranchEnUS | EarthlyBranchJaJP | EarthlyBranchKoKR | EarthlyBranchZhCN | EarthlyBranchZhTW | EarthlyBranchViVN;
    export type EarthlyBranchKey = keyof typeof earthlyBranchZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/FiveElementsClass" {
    import fiveElementsClassEnUS from "iztro/lib/i18n/locales/en-US/fiveElementsClass";
    import fiveElementsClassJaJP from "iztro/lib/i18n/locales/ja-JP/fiveElementsClass";
    import fiveElementsClassKoKR from "iztro/lib/i18n/locales/ko-KR/fiveElementsClass";
    import fiveElementsClassZhCN from "iztro/lib/i18n/locales/zh-CN/fiveElementsClass";
    import fiveElementsClassZhTW from "iztro/lib/i18n/locales/zh-TW/fiveElementsClass";
    import fiveElementsClassViVN from "iztro/lib/i18n/locales/vi-VN/fiveElementsClass";
    type FiveElementsClassEnUS = (typeof fiveElementsClassEnUS)[keyof typeof fiveElementsClassEnUS];
    type FiveElementsClassJaJP = (typeof fiveElementsClassJaJP)[keyof typeof fiveElementsClassJaJP];
    type FiveElementsClassKoKR = (typeof fiveElementsClassKoKR)[keyof typeof fiveElementsClassKoKR];
    type FiveElementsClassZhCN = (typeof fiveElementsClassZhCN)[keyof typeof fiveElementsClassZhCN];
    type FiveElementsClassZhTW = (typeof fiveElementsClassZhTW)[keyof typeof fiveElementsClassZhTW];
    type FiveElementsClassViVN = (typeof fiveElementsClassViVN)[keyof typeof fiveElementsClassViVN];
    export type FiveElementsClassName = FiveElementsClassEnUS | FiveElementsClassJaJP | FiveElementsClassKoKR | FiveElementsClassZhCN | FiveElementsClassZhTW | FiveElementsClassViVN;
    export type FiveElementsClassKey = keyof typeof fiveElementsClassZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/Gender" {
    import genderEnUS from "iztro/lib/i18n/locales/en-US/gender";
    import genderJaJP from "iztro/lib/i18n/locales/ja-JP/gender";
    import genderKoKR from "iztro/lib/i18n/locales/ko-KR/gender";
    import genderZhCN from "iztro/lib/i18n/locales/zh-CN/gender";
    import genderZhTW from "iztro/lib/i18n/locales/zh-TW/gender";
    import genderViVN from "iztro/lib/i18n/locales/vi-VN/gender";
    type GenderEnUS = (typeof genderEnUS)[keyof typeof genderEnUS];
    type GenderJaJP = (typeof genderJaJP)[keyof typeof genderJaJP];
    type GenderKoKR = (typeof genderKoKR)[keyof typeof genderKoKR];
    type GenderZhCN = (typeof genderZhCN)[keyof typeof genderZhCN];
    type GenderZhTW = (typeof genderZhTW)[keyof typeof genderZhTW];
    type GenderViVN = (typeof genderViVN)[keyof typeof genderViVN];
    export type GenderName = GenderEnUS | GenderJaJP | GenderKoKR | GenderZhCN | GenderZhTW | GenderViVN;
    export type GenderKey = keyof typeof genderZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/HeavenlyStem" {
    import heavenlyStemEnUS from "iztro/lib/i18n/locales/en-US/heavenlyStem";
    import heavenlyStemJaJP from "iztro/lib/i18n/locales/ja-JP/heavenlyStem";
    import heavenlyStemKoKR from "iztro/lib/i18n/locales/ko-KR/heavenlyStem";
    import heavenlyStemZhCN from "iztro/lib/i18n/locales/zh-CN/heavenlyStem";
    import heavenlyStemZhTW from "iztro/lib/i18n/locales/zh-TW/heavenlyStem";
    import heavenlyStemViVN from "iztro/lib/i18n/locales/vi-VN/heavenlyStem";
    type HeavenlyStemEnUS = (typeof heavenlyStemEnUS)[keyof typeof heavenlyStemEnUS];
    type HeavenlyStemJaJP = (typeof heavenlyStemJaJP)[keyof typeof heavenlyStemJaJP];
    type HeavenlyStemKoKR = (typeof heavenlyStemKoKR)[keyof typeof heavenlyStemKoKR];
    type HeavenlyStemZhCN = (typeof heavenlyStemZhCN)[keyof typeof heavenlyStemZhCN];
    type HeavenlyStemZhTW = (typeof heavenlyStemZhTW)[keyof typeof heavenlyStemZhTW];
    type HeavenlyStemViVN = (typeof heavenlyStemViVN)[keyof typeof heavenlyStemViVN];
    export type HeavenlyStemName = HeavenlyStemEnUS | HeavenlyStemJaJP | HeavenlyStemKoKR | HeavenlyStemZhCN | HeavenlyStemZhTW | HeavenlyStemViVN;
    export type HeavenlyStemKey = keyof typeof heavenlyStemZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/index" {
    export * from "iztro/lib/i18n/types/Brightness";
    export * from "iztro/lib/i18n/types/EarthlyBranch";
    export * from "iztro/lib/i18n/types/FiveElementsClass";
    export * from "iztro/lib/i18n/types/Gender";
    export * from "iztro/lib/i18n/types/HeavenlyStem";
    export * from "iztro/lib/i18n/types/Palace";
    export * from "iztro/lib/i18n/types/Star";
    export * from "iztro/lib/i18n/types/mutagen";
}

declare module "iztro/lib/i18n/types/mutagen" {
    import mutagenEnUS from "iztro/lib/i18n/locales/en-US/mutagen";
    import mutagenJaJP from "iztro/lib/i18n/locales/ja-JP/mutagen";
    import mutagenKoKR from "iztro/lib/i18n/locales/ko-KR/mutagen";
    import mutagenZhCN from "iztro/lib/i18n/locales/zh-CN/mutagen";
    import mutagenZhTW from "iztro/lib/i18n/locales/zh-TW/mutagen";
    import mutagenViVN from "iztro/lib/i18n/locales/vi-VN/mutagen";
    type MutagenEnUS = (typeof mutagenEnUS)[keyof typeof mutagenEnUS];
    type MutagenJaJP = (typeof mutagenJaJP)[keyof typeof mutagenJaJP];
    type MutagenKoKR = (typeof mutagenKoKR)[keyof typeof mutagenKoKR];
    type MutagenZhCN = (typeof mutagenZhCN)[keyof typeof mutagenZhCN];
    type MutagenZhTW = (typeof mutagenZhTW)[keyof typeof mutagenZhTW];
    type MutagenViVN = (typeof mutagenViVN)[keyof typeof mutagenViVN];
    export type Mutagen = MutagenEnUS | MutagenJaJP | MutagenKoKR | MutagenZhCN | MutagenZhTW | MutagenViVN;
    export type MutagenKey = keyof typeof mutagenZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/Palace" {
    import palaceEnUS from "iztro/lib/i18n/locales/en-US/palace";
    import palaceJaJP from "iztro/lib/i18n/locales/ja-JP/palace";
    import palaceKoKR from "iztro/lib/i18n/locales/ko-KR/palace";
    import palaceZhCN from "iztro/lib/i18n/locales/zh-CN/palace";
    import palaceZhTW from "iztro/lib/i18n/locales/zh-TW/palace";
    import palaceViVN from "iztro/lib/i18n/locales/vi-VN/palace";
    type PalaceEnUS = (typeof palaceEnUS)[keyof typeof palaceEnUS];
    type PalaceJaJP = (typeof palaceJaJP)[keyof typeof palaceJaJP];
    type PalaceKoKR = (typeof palaceKoKR)[keyof typeof palaceKoKR];
    type PalaceZhCN = (typeof palaceZhCN)[keyof typeof palaceZhCN];
    type PalaceZhTW = (typeof palaceZhTW)[keyof typeof palaceZhTW];
    type PalaceViVN = (typeof palaceViVN)[keyof typeof palaceViVN];
    export type PalaceName = PalaceEnUS | PalaceJaJP | PalaceKoKR | PalaceZhCN | PalaceZhTW | PalaceViVN;
    export type PalaceKey = keyof typeof palaceZhCN;
    export {};
}

declare module "iztro/lib/i18n/types/Star" {
    import starEnUS from "iztro/lib/i18n/locales/en-US/star";
    import starJaJP from "iztro/lib/i18n/locales/ja-JP/star";
    import starKoKR from "iztro/lib/i18n/locales/ko-KR/star";
    import starZhCN from "iztro/lib/i18n/locales/zh-CN/star";
    import starZhTW from "iztro/lib/i18n/locales/zh-TW/star";
    import starViVN from "iztro/lib/i18n/locales/vi-VN/star";
    type StarEnUS = (typeof starEnUS)[keyof typeof starEnUS];
    type StarJaJP = (typeof starJaJP)[keyof typeof starJaJP];
    type StarKoKR = (typeof starKoKR)[keyof typeof starKoKR];
    type StarZhCN = (typeof starZhCN)[keyof typeof starZhCN];
    type StarZhTW = (typeof starZhTW)[keyof typeof starZhTW];
    type StarViVN = (typeof starViVN)[keyof typeof starViVN];
    export type StarName = StarEnUS | StarJaJP | StarKoKR | StarZhCN | StarZhTW | StarViVN;
    export type StarKey = keyof typeof starZhCN;
    export {};
}

declare module "iztro/lib/star/adjectiveStar" {
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    import { AstrolabeParam } from "iztro/lib/data/types/index";
    /**
     * 安杂耀
     *
     * @param {AstrolabeParam} param - 通用排盘参数参数
     * @returns 38杂耀
     */
    export const getAdjectiveStar: (param: AstrolabeParam) => FunctionalStar[][];
}

declare module "iztro/lib/star/decorativeStar" {
    import { StarName, EarthlyBranchName, FiveElementsClassName, GenderName } from "iztro/lib/i18n/index";
    import { AstrolabeParam } from "iztro/lib/data/types/index";
    /**
     * 获取长生12神开始的宫位索引
     *
     * - 水二局长生在申
     * - 木三局长生在亥
     * - 金四局长生在巳
     * - 土五局长生在申
     * - 火六局长生在寅，
     * @param fiveElementClassName 五行局
     * @returns 长生12神开始的索引
     */
    export const getChangesheng12StartIndex: (fiveElementClassName: FiveElementsClassName) => number;
    /**
     * 长生12神。
     *
     * 阳男阴女顺行，阴男阳女逆行，安长生、沐浴、冠带、临官、帝旺、衰、病、死、墓、绝 、胎、养。
     *
     * @param {AstrolabeParam} param 通用排盘参数
     * @returns 长生12神从寅宫开始的顺序
     */
    export const getchangsheng12: (param: AstrolabeParam) => StarName[];
    /**
     * 博士12神。
     *
     * 从禄存起，阳男阴女顺行，阴男阳女逆行。安博士、力士、青龙、小耗、将军、奏书、飞廉、喜神、病符、大耗、伏兵、官府。
     *
     * @param solarDateStr 阳历日期字符串
     * @param gender 性别【男｜女】
     * @returns 博士12神从寅宫开始的顺序
     */
    export const getBoShi12: (solarDateStr: string, gender: GenderName) => StarName[];
    /**
     * 安流年将前诸星（按流年地支起将星）
     * - 寅午戍年将星午，申子辰年子将星，
     * - 巳酉丑将酉上驻，亥卯未将卯上停。
     * - 攀鞍岁驿并息神，华盖劫煞灾煞轻，
     * - 天煞指背咸池续，月煞亡神次第行。
     *
     * @param earthlyBranchName 地支
     * @returns 将前诸星起始索引
     */
    export const getJiangqian12StartIndex: (earthlyBranchName: EarthlyBranchName) => number;
    /**
     * 流年诸星。
     *
     * - 流年岁前诸星
     *   - 流年地支起岁建，岁前首先是晦气，
     *   - 丧门贯索及官符，小耗大耗龙德继，
     *   - 白虎天德连吊客，病符居后须当记。
     *
     * - 安流年将前诸星（按流年地支起将星）
     *   - 寅午戍年将星午，申子辰年子将星，
     *   - 巳酉丑将酉上驻，亥卯未将卯上停。
     *   - 攀鞍岁驿并息神，华盖劫煞灾煞轻，
     *   - 天煞指背咸池续，月煞亡神次第行。
     *
     * @param solarDateStr 阳历日期字符串
     * @returns 流年诸星从寅宫开始的顺序
     */
    export const getYearly12: (solarDateStr: string | Date) => {
        suiqian12: StarName[];
        jiangqian12: StarName[];
    };
}

declare module "iztro/lib/star/FunctionalStar" {
    import { IFunctionalAstrolabe } from "iztro/lib/astro/FunctionalAstrolabe";
    import { IFunctionalPalace } from "iztro/lib/astro/FunctionalPalace";
    import { IFunctionalSurpalaces } from "iztro/lib/astro/FunctionalSurpalaces";
    import { Star } from "iztro/lib/data/types/index";
    import { Brightness, Mutagen } from "iztro/lib/i18n/index";
    /**
     * 星耀类的接口定义
     *
     * 文档地址：https://docs.iztro.com/posts/star.html#functionalstar
     */
    export interface IFunctionalStar extends Star {
        /**
         * 获取星耀所在宫位
         *
         * @version v1.2.0
         *
         * @returns 星耀所在宫位
         */
        palace: () => IFunctionalPalace | undefined;
        /**
         * 设置当前星耀所在宫位
         *
         * @param p 宫位的实例
         */
        setPalace: (p: IFunctionalPalace) => void;
        /**
         * 设置当前星耀所在星盘
         *
         * @version v1.2.0
         *
         * @param a 星盘实例
         */
        setAstrolabe: (a: IFunctionalAstrolabe) => void;
        /**
         * 获取当前星耀的三方四正宫位
         *
         * @version v1.2.0
         *
         * @returns 三方四正宫位 ｜ undefined
         */
        surroundedPalaces: () => IFunctionalSurpalaces | undefined;
        /**
         * 获取当前星耀的对宫
         *
         * @version v1.2.0
         *
         * @returns 对宫 ｜ undefined
         */
        oppositePalace: () => IFunctionalPalace | undefined;
        /**
         * 判断星耀是否是传入的亮度，也可以传入多个亮度，只要匹配到一个亮度就会返回 `true`
         *
         * @version v1.2.0
         *
         * @param brightness 星耀亮度
         * @returns true | false
         */
        withBrightness: (brightness: Brightness | Brightness[]) => boolean;
        /**
         * 判断星耀是否产生了四化
         *
         * @version v1.2.0
         *
         * @param mutagen 四化【禄|权|科|忌】
         * @returns true | false
         */
        withMutagen: (mutagen: Mutagen | Mutagen[]) => boolean;
    }
    /**
     * 星耀类。
     *
     * 文档地址：https://docs.iztro.com/posts/star.html
     */
    export default class FunctionalStar implements IFunctionalStar {
        name: import("iztro/lib/i18n/index").StarName;
        type: import("iztro/lib/data/types/index").StarType;
        scope: import("iztro/lib/data/types/index").Scope;
        brightness?: Brightness | undefined;
        mutagen?: Mutagen | undefined;
        private _palace?;
        private _astrolabe?;
        constructor(data: Star);
        oppositePalace: () => IFunctionalPalace | undefined;
        setPalace: (p: IFunctionalPalace) => void;
        setAstrolabe: (a: IFunctionalAstrolabe) => void;
        palace: () => IFunctionalPalace | undefined;
        surroundedPalaces: () => IFunctionalSurpalaces | undefined;
        withMutagen: (mutagen: Mutagen | Mutagen[]) => boolean;
        withBrightness: (brightness: Brightness | Brightness[]) => boolean;
    }
}

declare module "iztro/lib/star/horoscopeStar" {
    import { Scope } from "iztro/lib/data/types/index";
    import { HeavenlyStemName, EarthlyBranchName } from "iztro/lib/i18n/index";
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    /**
     * 获取流耀
     *
     * 魁钺昌曲禄羊陀马鸾喜
     *
     * @param heavenlyStem 天干
     * @param earthlyBranch 地支
     */
    export const getHoroscopeStar: (heavenlyStem: HeavenlyStemName, earthlyBranch: EarthlyBranchName, scope: Scope) => FunctionalStar[][];
}

declare module "iztro/lib/star/index" {
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    export const initStars: () => FunctionalStar[][];
    export * from "iztro/lib/star/location";
    export * from "iztro/lib/star/majorStar";
    export * from "iztro/lib/star/minorStar";
    export * from "iztro/lib/star/adjectiveStar";
    export * from "iztro/lib/star/decorativeStar";
    export * from "iztro/lib/star/horoscopeStar";
}

declare module "iztro/lib/star/location" {
    import { EarthlyBranchKey, EarthlyBranchName, HeavenlyStemName } from "iztro/lib/i18n/index";
    import { AstrolabeParam } from "iztro/lib/data/types/index";
    /**
     * 起紫微星诀算法
     *
     * - 六五四三二，酉午亥辰丑，
     * - 局数除日数，商数宫前走；
     * - 若见数无余，便要起虎口，
     * - 日数小於局，还直宫中守。
     *
     * 举例：
     * - 例一：27日出生木三局，以三除27，循环0次就可以整除，27➗3=9，从寅进9格，在戍安紫微。
     * - 例二：13日出生火六局，以六除13，最少需要加5才能整除， 18➗8=3，从寅进3格为辰，添加数为5（奇数），故要逆回五宫，在亥安紫微。
     * - 例三：6日出生土五局，以五除6，最少需要加4才能整除，10➗5=2，从寅进2格为卯，添加数为4（偶数），顺行4格为未，在未安紫微。
     *
     * @param solarDateStr 公历日期 YYYY-MM-DD
     * @param timeIndex 时辰索引【0～12】
     * @param fixLeap 是否调整农历闰月（若该月不是闰月则不会生效）
     * @param from 根据传入的干支起五行局来计算紫微星和天府星位置
     * @returns 紫微和天府星所在宫位索引
     */
    export const getStartIndex: (param: AstrolabeParam) => {
        ziweiIndex: number;
        tianfuIndex: number;
    };
    /**
     * 按年干支计算禄存、擎羊，陀罗、天马的索引
     *
     * 定禄存、羊、陀诀（按年干）
     *
     * - 甲禄到寅宫，乙禄居卯府。
     * - 丙戊禄在巳，丁己禄在午。
     * - 庚禄定居申，辛禄酉上补。
     * - 壬禄亥中藏，癸禄居子户。
     * - 禄前羊刃当，禄后陀罗府。
     *
     * 安天马（按年支），天马只会出现在四马地【寅申巳亥】
     *
     * - 寅午戍流马在申，申子辰流马在寅。
     * - 巳酉丑流马在亥，亥卯未流马在巳。
     *
     * @param heavenlyStemName 天干
     * @param earthlyBranchName 地支
     * @returns 禄存、擎羊，陀罗、天马的索引
     */
    export const getLuYangTuoMaIndex: (heavenlyStemName: HeavenlyStemName, earthlyBranchName: EarthlyBranchName) => {
        luIndex: number;
        maIndex: number;
        yangIndex: number;
        tuoIndex: number;
    };
    /**
     * 获取天魁天钺所在宫位索引（按年干）
     *
     * - 甲戊庚之年丑未
     * - 乙己之年子申
     * - 辛年午寅
     * - 壬癸之年卯巳
     * - 丙丁之年亥酉
     *
     * @param heavenlyStemName 天干
     * @returns
     */
    export const getKuiYueIndex: (heavenlyStemName: HeavenlyStemName) => {
        kuiIndex: number;
        yueIndex: number;
    };
    /**
     * 获取左辅右弼的索引（按生月）
     *
     * - 辰上顺正寻左辅
     * - 戌上逆正右弼当
     *
     * 解释：
     *
     * - 从辰顺数农历月份数是左辅的索引
     * - 从戌逆数农历月份数是右弼的索引
     *
     * @param lunarMonth 农历月份
     * @returns 左辅、右弼索引
     */
    export const getZuoYouIndex: (lunarMonth: number) => {
        zuoIndex: number;
        youIndex: number;
    };
    /**
     * 获取文昌文曲的索引（按时支）
     *
     * - 辰上顺时文曲位
     * - 戌上逆时觅文昌
     *
     * 解释：
     *
     * - 从辰顺数到时辰地支索引是文曲的索引
     * - 从戌逆数到时辰地支索引是文昌的索引
     *
     * 由于时辰地支的索引即是时辰的序号，所以可以直接使用时辰的序号
     *
     * @param timeIndex 时辰索引【0～12】
     * @returns 文昌、文曲索引
     */
    export const getChangQuIndex: (timeIndex: number) => {
        changIndex: number;
        quIndex: number;
    };
    /**
     * 获取日系星索引，包括
     *
     * 三台，八座，恩光，天贵
     *
     * - 安三台八座
     *   - 由左辅之宫位起初一，顺行至生日安三台。
     *   - 由右弼之宫位起初一，逆行至生日安八座。
     *
     * - 安恩光天贵
     *   - 由文昌之宫位起初一，顺行至生日再退一步起恩光。
     *   - 由文曲之宫位起初一，顺行至生日再退一步起天贵。
     *
     * @param solarDateStr 阳历日期
     * @param timeIndex 时辰索引【0～12】
     * @returns 三台，八座索引
     */
    export const getDailyStarIndex: (solarDateStr: string, timeIndex: number, fixLeap?: boolean) => {
        santaiIndex: number;
        bazuoIndex: number;
        enguangIndex: number;
        tianguiIndex: number;
    };
    /**
     * 获取时系星耀索引，包括台辅，封诰
     *
     * @param timeIndex 时辰序号【0～12】
     * @returns 台辅，封诰索引
     */
    export const getTimelyStarIndex: (timeIndex: number) => {
        taifuIndex: number;
        fenggaoIndex: number;
    };
    /**
     * 获取地空地劫的索引（按时支）
     *
     * - 亥上子时顺安劫
     * - 逆回便是地空亡
     *
     * 解释：
     *
     * - 从亥顺数到时辰地支索引是地劫的索引
     * - 从亥逆数到时辰地支索引是地空的索引
     *
     * 由于时辰地支的索引即是时辰的序号，所以可以直接使用时辰的序号
     *
     * @param timeIndex 时辰索引【0～12】
     * @returns 地空、地劫索引
     */
    export const getKongJieIndex: (timeIndex: number) => {
        kongIndex: number;
        jieIndex: number;
    };
    /**
     * 获取火星铃星索引（按年支以及时支）
     *
     * - 申子辰人寅戌扬
     * - 寅午戌人丑卯方
     * - 巳酉丑人卯戌位
     * - 亥卯未人酉戌房
     *
     * 起火铃二耀先据出生年支，依口诀定火铃起子时位。
     *
     * 例如壬辰年卯时生人，据[申子辰人寅戌扬]口诀，故火星在寅宫起子时，铃星在戌宫起子时，顺数至卯时，即火星在巳，铃星在丑。
     *
     * @param earthlyBranchName 地支
     * @param timeIndex 时辰序号
     * @returns 火星、铃星索引
     */
    export const getHuoLingIndex: (earthlyBranchName: EarthlyBranchName, timeIndex: number) => {
        huoIndex: number;
        lingIndex: number;
    };
    /**
     * 获取红鸾天喜所在宫位索引
     *
     * - 卯上起子逆数之
     * - 数到当生太岁支
     * - 坐守此宫红鸾位
     * - 对宫天喜不差移
     *
     * @param earthlyBranchName 年支
     * @returns 红鸾、天喜索引
     */
    export const getLuanXiIndex: (earthlyBranchName: EarthlyBranchName) => {
        hongluanIndex: number;
        tianxiIndex: number;
    };
    /**
     * 安华盖
     * - 子辰申年在辰，丑巳酉年在丑
     * - 寅午戍年在戍，卯未亥年在未。
     *
     * 安咸池
     * - 子辰申年在酉，丑巳酉年在午
     * - 寅午戍年在卯，卯未亥年在子。
     *
     * @param earthlyBranchName 地支
     * @returns 华盖、咸池索引
     */
    export const getHuagaiXianchiIndex: (earthlyBranchName: EarthlyBranchName) => {
        huagaiIndex: number;
        xianchiIndex: number;
    };
    /**
     * 安孤辰寡宿
     * - 寅卯辰年安巳丑
     * - 巳午未年安申辰
     * - 申酉戍年安亥未
     * - 亥子丑年安寅戍。
     *
     * @param earthlyBranchName 地支
     * @returns 孤辰、寡宿索引
     */
    export const getGuGuaIndex: (earthlyBranchName: EarthlyBranchName) => {
        guchenIndex: number;
        guasuIndex: number;
    };
    /**
     * 安劫杀诀（年支）
     * 申子辰人蛇开口、亥卯未人猴速走
     * 寅午戌人猪面黑、巳酉丑人虎咆哮
     *
     * @version v2.5.0
     *
     * @param earthlyBranchKey 生年地支
     * @returns {number} 劫杀索引
     */
    export const getJieshaAdjIndex: (earthlyBranchKey: EarthlyBranchKey) => 0 | 3 | 6 | 9;
    /**
     * 安大耗诀（年支）
     * 但用年支去对冲、阴阳移位过一宫
     * 阳顺阴逆移其位、大耗原来不可逢
     *
     * 大耗安法，是在年支之对宫，前一位或后一位安星。阳支顺行前一位，阴支逆行后一位。
     *
     * @param earthlyBranchKey 生年地支
     * @returns {number} 大耗索引
     */
    export const getDahaoIndex: (earthlyBranchKey: EarthlyBranchKey) => number;
    /**
     * 获取年系星的索引，包括
     * 咸池，华盖，孤辰，寡宿, 天厨，破碎，天才，天寿，蜚蠊, 龙池，凤阁，天哭，天虚，
     * 天官，天福
     *
     * - 安天才天寿
     *   - 天才由命宫起子，顺行至本生年支安之。天寿由身宫起子，顺行至本生年支安之。
     *
     * - 安破碎
     *   - 子午卯酉年安巳宫，寅申巳亥年安酉宫，辰戍丑未年安丑宫。
     *
     * - 安天厨
     *   - 甲丁食蛇口，乙戊辛马方。丙从鼠口得，己食于猴房。庚食虎头上，壬鸡癸猪堂。
     *
     * - 安蜚蠊
     *   - 子丑寅年在申酉戍，卯辰巳年在巳午未，午未申年在寅卯辰，酉戍亥年在亥子丑。
     *
     * - 安龙池凤阁
     *   - 龙池从辰宫起子，顺至本生年支安之。凤阁从戍宫起子，逆行至本生年支安之。
     *
     * - 安天哭天虚
     *   - 天哭天虚起午宫，午宫起子两分踪，哭逆行兮虚顺转，数到生年便停留。
     *
     * - 安天官天福
     *   - 甲喜羊鸡乙龙猴，丙年蛇鼠一窝谋。丁虎擒猪戊玉兔，
     *   - 己鸡居然与虎俦。庚猪马辛鸡蛇走，壬犬马癸马蛇游。
     *
     * - 安截路空亡（截空）
     *   - 甲己之年申酉，乙庚之年午未，
     *   - 丙辛之年辰巳，丁壬之年寅卯，
     *   - 戊癸之年子丑。
     *
     * - 安天空
     *   - 生年支顺数的前一位就是。
     * @param solarDate 阳历日期
     * @param timeIndex 时辰序号
     * @param gender 性别
     * @param fixLeap 是否修复闰月，假如当月不是闰月则不生效
     */
    export const getYearlyStarIndex: (param: AstrolabeParam) => {
        xianchiIndex: number;
        huagaiIndex: number;
        guchenIndex: number;
        guasuIndex: number;
        tiancaiIndex: number;
        tianshouIndex: number;
        tianchuIndex: number;
        posuiIndex: number;
        feilianIndex: number;
        longchiIndex: number;
        fenggeIndex: number;
        tiankuIndex: number;
        tianxuIndex: number;
        tianguanIndex: number;
        tianfuIndex: number;
        tiandeIndex: number;
        yuedeIndex: number;
        tiankongIndex: number;
        jieluIndex: number;
        kongwangIndex: number;
        xunkongIndex: number;
        tianshangIndex: number;
        tianshiIndex: number;
        jiekongIndex: number;
        jieshaAdjIndex: number;
        nianjieIndex: number;
        dahaoAdjIndex: number;
    };
    /**
     * 获取年解的索引
     *
     * - 年解（按年支）
     *   - 解神从戌上起子，逆数至当生年太岁上是也
     *
     * @param earthlyBranch 地支（年）
     * @returns 年解索引
     */
    export const getNianjieIndex: (earthlyBranchName: EarthlyBranchName) => number;
    /**
     * 获取以月份索引为基准的星耀索引，包括解神，天姚，天刑，阴煞，天月，天巫
     * 解神分为年解和月解，月解作用更加直接快速，年解稍迟钝，且作用力没有月解那么大
     *
     * - 月解（按生月）
     *   - 正二在申三四在戍，五六在子七八在寅，九十月坐於辰宫，十一十二在午宫。
     *
     * - 安天刑天姚（三合必见）
     *   - 天刑从酉起正月，顺至生月便安之。天姚丑宫起正月，顺到生月即停留。
     *
     * - 安阴煞
     *   - 正七月在寅，二八月在子，三九月在戍，四十月在申，五十一在午，六十二在辰。
     *
     * - 安天月
     *   - 一犬二蛇三在龙，四虎五羊六兔宫。七猪八羊九在虎，十马冬犬腊寅中。
     *
     * - 安天巫
     *   - 正五九月在巳，二六十月在申，三七十一在寅，四八十二在亥。
     *
     * @param solarDate 阳历日期
     * @param timeIndex 时辰序号
     * @param fixLeap 是否修复闰月，假如当月不是闰月则不生效
     * @returns
     */
    export const getMonthlyStarIndex: (solarDate: string, timeIndex: number, fixLeap?: boolean) => {
        yuejieIndex: number;
        tianyaoIndex: number;
        tianxingIndex: number;
        yinshaIndex: number;
        tianyueIndex: number;
        tianwuIndex: number;
    };
    /**
     * 通过 大限/流年 天干获取流昌流曲
     *
     * - 流昌起巳位	甲乙顺流去
     * - 不用四墓宫	日月同年岁
     * - 流曲起酉位	甲乙逆行踪
     * - 亦不用四墓	年日月相同
     *
     * @param heavenlyStemName 天干
     * @returns 文昌、文曲索引
     */
    export const getChangQuIndexByHeavenlyStem: (heavenlyStemName: HeavenlyStemName) => {
        changIndex: number;
        quIndex: number;
    };
}

declare module "iztro/lib/star/majorStar" {
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    import { AstrolabeParam } from "iztro/lib/data/types/index";
    /**
     * 安主星，寅宫下标为0，若下标对应的数组为空数组则表示没有星耀
     *
     * 安紫微诸星诀
     * - 紫微逆去天机星，隔一太阳武曲辰，
     * - 连接天同空二宫，廉贞居处方是真。
     *
     * 安天府诸星诀
     * - 天府顺行有太阴，贪狼而后巨门临，
     * - 随来天相天梁继，七杀空三是破军。
     *
     * @param {AstrolabeParam} param 通用排盘参数
     * @returns {Array<Star[]>} 从寅宫开始每一个宫的星耀
     */
    export const getMajorStar: (param: AstrolabeParam) => FunctionalStar[][];
}

declare module "iztro/lib/star/minorStar" {
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    /**
     * 安14辅星，寅宫下标为0，若下标对应的数组为空数组则表示没有星耀
     *
     * @param solarDateStr 阳历日期字符串
     * @param timeIndex 时辰索引【0～12】
     * @param fixLeap 是否修复闰月，假如当月不是闰月则不生效
     * @returns 14辅星
     */
    export const getMinorStar: (solarDateStr: string, timeIndex: number, fixLeap?: boolean) => FunctionalStar[][];
}

declare module "iztro/lib/utils/index" {
    import { Brightness, EarthlyBranchName, HeavenlyStemName, Mutagen, StarName } from "iztro/lib/i18n/index";
    import FunctionalStar from "iztro/lib/star/FunctionalStar";
    import { HeavenlyStemAndEarthlyBranchDate } from "lunar-lite/lib/types";
    /**
     * 用于处理索引，将索引锁定在 0~max 范围内
     *
     * @param index 当前索引
     * @param max 最大循环数，默认为12【因为12用得最多，宫位数量以及十二地支数量都为12，所以将12作为默认值】
     * @returns {number} 处理后的索引
     */
    export const fixIndex: (index: number, max?: number) => number;
    /**
     * 因为宫位是从寅宫开始的排列的，所以需要将目标地支的序号减去寅的序号才能得到宫位的序号
     *
     * @param {EarthlyBranchName} earthlyBranch 地支
     * @returns {number} 该地支对应的宫位索引序号
     */
    export const earthlyBranchIndexToPalaceIndex: (earthlyBranchName: EarthlyBranchName) => number;
    /**
     * 配置星耀亮度
     *
     * @param {StarName} starName 星耀名字
     * @param {number} index 所在宫位索引
     */
    export const getBrightness: (starName: StarName, index: number) => Brightness;
    export const getMutagen: (starName: StarName, heavenlyStemName: HeavenlyStemName) => Mutagen;
    export const getMutagensByHeavenlyStem: (heavenlyStemName: HeavenlyStemName) => StarName[];
    /**
     * 处理地支相对于十二宫的索引，因为十二宫是以寅宫开始，所以下标需要减去地支寅的索引
     *
     * @param {EarthlyBranchName} earthlyBranch 地支
     * @returns {number} Number(0~11)
     */
    export const fixEarthlyBranchIndex: (earthlyBranchName: EarthlyBranchName) => number;
    /**
     * 调整农历月份的索引
     *
     * 正月建寅（正月地支为寅），fixLeap为是否调整闰月情况
     * 若调整闰月，则闰月的前15天按上月算，后面天数按下月算
     * 比如 闰二月 时，fixLeap 为 true 时 闰二月十五(含)前
     * 的月份按二月算，之后的按三月算
     *
     * @param {string} solarDateStr 阳历日期
     * @param {number} timeIndex 时辰序号
     * @param {vboolean} fixLeap 是否调整闰月
     * @returns {number} 月份索引
     */
    export const fixLunarMonthIndex: (solarDateStr: string, timeIndex: number, fixLeap?: boolean) => number;
    /**
     * 获取农历日期【天】的索引，晚子时将加一天，所以如果是晚子时下标不需要减一
     *
     * @param lunarDay 农历日期【天】
     * @param timeIndex 时辰索引
     * @returns {number} 农历日期【天】
     */
    export const fixLunarDayIndex: (lunarDay: number, timeIndex: number) => number;
    /**
     * 将多个星耀数组合并到一起
     *
     * @param {FunctionalStar[][][]} stars 星耀数组
     * @returns {FunctionalStar[][]} 合并后的星耀
     */
    export const mergeStars: (...stars: FunctionalStar[][][]) => FunctionalStar[][];
    /**
     * 将时间的小时转化为时辰的索引
     *
     * @param {number} hour 当前时间的小时数
     * @returns {number} 时辰的索引
     */
    export const timeToIndex: (hour: number) => number;
    /**
     * 起小限
     *
     * - 小限一年一度逢，男顺女逆不相同，
     * - 寅午戍人辰上起，申子辰人自戍宫，
     * - 巳酉丑人未宫始，亥卯未人起丑宫。
     *
     * @param {EarthlyBranchName} earthlyBranchName 地支
     * @returns {number} 小限开始的宫位索引
     */
    export const getAgeIndex: (earthlyBranchName: EarthlyBranchName) => number;
    /**
     * 返回翻译后的干支纪年字符串
     *
     * @param chineseDate 干支纪年日期对象
     * @returns 干支纪年字符串
     */
    export const translateChineseDate: (chineseDate: HeavenlyStemAndEarthlyBranchDate) => string;
}

declare module "lunar-lite/lib/constants" {
    /**
     * 天干速查表
     */
    export const HEAVENLY_STEMS: readonly [
        "甲",
        "乙",
        "丙",
        "丁",
        "戊",
        "己",
        "庚",
        "辛",
        "壬",
        "癸"
    ];
    /**
     * 地支速查表
     */
    export const EARTHLY_BRANCHES: readonly [
        "子",
        "丑",
        "寅",
        "卯",
        "辰",
        "巳",
        "午",
        "未",
        "申",
        "酉",
        "戌",
        "亥"
    ];
    /** 十二生肖（按地支顺序） */
    export const ZODIAC: readonly [
        "鼠",
        "牛",
        "虎",
        "兔",
        "龙",
        "蛇",
        "马",
        "羊",
        "猴",
        "鸡",
        "狗",
        "猪"
    ];
}

declare module "lunar-lite/lib/types" {
    import { EARTHLY_BRANCHES, HEAVENLY_STEMS } from "lunar-lite/lib/constants";
    /**
     * 农历日期对象
     *
     * @property
     * - lunarYear 年
     * - lunarMonth 月
     * - lunarDay 日
     * - isLeap 月份是否闰月
     *
     * @function toString() 输出 YYYY-M-D 或 农历中文 字符串
     */
    export type LunarDate = {
        /** 农历年 */
        lunarYear: number;
        /** 农历月 */
        lunarMonth: number;
        /** 农历日 */
        lunarDay: number;
        /** 是否闰月 */
        isLeap: boolean;
        /**
         * 转化为字符串
         *
         * @param toCnStr 是否使用中文字符串, 若该参数为false则字符串中不会携带闰月信息
         * @returns string
         * @example
         * lunarYear = 2023;
         * lunarMonth = 6;
         * lunarDay = 12;
         * isLeap = true;
         *
         * toString(); // 2023-6-12
         * toString(true); // 二〇二三年闰二月十一
         */
        toString: (toCnStr?: boolean) => string;
    };
    /**
     * 阳历日期对象
     *
     * @property
     * - solarYear 年
     * - solarMonth 月
     * - solarDay 日
     *
     * @function toString() 将对象以 YYYY-M-D 格式字符串输出
     */
    export type SolarDate = {
        /** 公历年 */
        solarYear: number;
        /** 公历月 */
        solarMonth: number;
        /** 公历日 */
        solarDay: number;
        /**
         * 转化为字符串
         *
         * @returns string
         * @example
         * solarYear = 2023;
         * solarMonth = 6;
         * solarDay = 12;
         *
         * toString(); // 2023-6-12
         */
        toString: () => string;
    };
    export type HeavenlyStem = (typeof HEAVENLY_STEMS)[number];
    export type EarthlyBranch = (typeof EARTHLY_BRANCHES)[number];
    export type HeavenlyStemAndEarthlyBranch = [
        HeavenlyStem,
        EarthlyBranch
    ];
    export type HeavenlyStemAndEarthlyBranchDate = {
        /** 年柱[天干，地支] */
        yearly: HeavenlyStemAndEarthlyBranch;
        /** 月柱[天干，地支] */
        monthly: HeavenlyStemAndEarthlyBranch;
        /** 日柱[天干，地支] */
        daily: HeavenlyStemAndEarthlyBranch;
        /** 时柱[天干，地支] */
        hourly: HeavenlyStemAndEarthlyBranch;
        /**
         * 获取四柱的字符串
         *
         * @returns 四柱字符串，用空格隔开
         * @example
         * yearly = ['癸', '卯'];
         * monthly = ['戊', '午'];
         * daily = ['癸', '亥'];
         * hourly = ['甲', '寅'];
         * toString(); // 癸卯 戊午 癸亥 甲寅
         */
        toString: () => string;
    };
    export type Options = {
        /** exact：立春分界，normal：除夕分界 */
        year?: "exact" | "normal";
        /** exact: 获取阴历月的干支（新的一月以节交接当天零点起算）, normal: 获取阴历月的干支（新的一月以节交接准确时刻起算） */
        month?: "exact" | "normal";
    };
}
