import typescript from '@rollup/plugin-typescript'

export default {
  input: 'guest-js/index.ts',
  output: [
    { file: 'dist-js/index.js', format: 'es' },
    { file: 'dist-js/index.cjs', format: 'cjs' },
  ],
  plugins: [typescript({ tsconfig: './tsconfig.json' })],
}
