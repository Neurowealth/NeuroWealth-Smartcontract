/**
 * Jest configuration for the monitoring package (#701).
 *
 * The package referenced jest in its scripts and devDependencies but shipped no
 * config and no tests, so `npm test` failed with "no tests found" and the four
 * areas the issue lists were entirely uncovered. ts-jest compiles the sources
 * through the package's own tsconfig, so the tests run against the same target
 * and module mode the package actually ships.
 */
module.exports = {
  preset: "ts-jest",
  testEnvironment: "node",
  roots: ["<rootDir>/src"],
  testMatch: ["**/__tests__/**/*.test.ts"],
  clearMocks: true,
  collectCoverageFrom: ["src/**/*.ts", "!src/**/*.test.ts"],
};
