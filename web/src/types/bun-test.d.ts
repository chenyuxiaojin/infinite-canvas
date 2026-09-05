declare module "bun:test" {
    export const describe: (name: string, run: () => void) => void;
    export const test: (name: string, run: () => void | Promise<void>) => void;
    export const expect: (value: unknown) => {
        toHaveLength: (expected: number) => void;
        toBe: (expected: unknown) => void;
        toBeTrue: () => void;
        toBeFalse: () => void;
        toBeUndefined: () => void;
        toContain: (expected: string) => void;
        toEqual: (expected: unknown) => void;
        toMatchObject: (expected: unknown) => void;
    };
}
