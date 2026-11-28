import react from '@vitejs/plugin-react'
import { configDefaults, defineConfig } from 'vitest/config'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    // Run exactly what `tsc -b` checks: .ts/.tsx tests outside dot-folders and dot-files.
    include: ['**/*.{test,spec}.{ts,tsx}'],
    exclude: [...configDefaults.exclude, '**/.*/**', '**/.*', '**/node_modules/**', '**/dist/**'],
  },
})
