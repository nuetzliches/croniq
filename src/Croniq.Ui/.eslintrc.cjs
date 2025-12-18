/** @type {import('eslint').Linter.Config} */
module.exports = {
  root: true,
  ignorePatterns: [
    'out-tsc/**',
    'dist/**',
    'coverage/**',
    'node_modules/**',
    'projects/api-schema/generated/**',
    '**/*.generated.*',
  ],
  overrides: [
    {
      files: ['src/**/*.ts', 'projects/**/src/**/*.ts'],
      parser: '@typescript-eslint/parser',
      parserOptions: {
        project: [
          'tsconfig.eslint.json',
        ],
        tsconfigRootDir: __dirname,
        sourceType: 'module',
      },
      plugins: ['@typescript-eslint', '@angular-eslint'],
      extends: [
        'eslint:recommended',
        'plugin:@typescript-eslint/recommended',
        'plugin:@angular-eslint/recommended',
      ],
      rules: {
        '@angular-eslint/component-class-suffix': 'off',
        '@typescript-eslint/no-floating-promises': 'error',
        '@typescript-eslint/no-misused-promises': 'error',
        '@typescript-eslint/no-unused-vars': [
          'error',
          { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
        ],
        'object-curly-newline': [
          'error',
          {
            ImportDeclaration: 'never',
            ExportDeclaration: 'never',
          },
        ],
        'no-restricted-imports': [
          'error',
          {
            paths: [
              {
                name: '@angular/common',
                importNames: ['CommonModule'],
                message: 'Avoid CommonModule in standalone components; import the standalone directives/pipes you use (e.g. DatePipe) instead.',
              },
            ],
            patterns: ['../**'],
          },
        ],
        'padding-line-between-statements': [
          'error',
          { blankLine: 'never', prev: 'import', next: 'import' },
        ],
      },
    },
    {
      files: ['projects/api-schema/src/**/*.ts'],
      rules: {
        'no-restricted-imports': 'off',
      },
    },
    {
      files: ['tools/**/*.ts', 'config/**/*.ts', '*.ts'],
      parser: '@typescript-eslint/parser',
      parserOptions: {
        tsconfigRootDir: __dirname,
        sourceType: 'module',
      },
      plugins: ['@typescript-eslint'],
      extends: ['eslint:recommended', 'plugin:@typescript-eslint/recommended'],
      rules: {
        '@typescript-eslint/no-floating-promises': 'off',
        '@typescript-eslint/no-misused-promises': 'off',
      },
    },
    {
      files: ['*.html'],
      extends: ['plugin:@angular-eslint/template/recommended'],
      parser: '@angular-eslint/template-parser',
      rules: {},
    },
  ],
};
