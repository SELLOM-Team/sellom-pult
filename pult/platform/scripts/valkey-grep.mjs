#!/usr/bin/env node
/**
 * Гейт AD-28: сервер кэша — только Valkey; docker-образ конкурента запрещён.
 *
 * Сканирует все `*.yml`, `*.yaml` и `Dockerfile*` репозитория:
 *  - YAML: строки `image:` — значение не должно содержать слова-названия
 *    запрещённого кэш-сервера (word-boundary, регистронезависимо);
 *  - Dockerfile: строки `FROM ...` — тот же запрет.
 * Образы valkey/valkey не дают ложных срабатываний.
 *
 * При находке печатает файл:строку и завершается с ненулевым кодом.
 *
 * Запуск: node platform/scripts/valkey-grep.mjs [корень репозитория]
 */

import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// Само имя запрещённого сервера пишется динамически, чтобы гейт не нашёл
// его в собственных исходниках (валидация секретов не для этого, а гигиена).
const FORBIDDEN = ['red', 'is'].join('')

const SKIP_DIRS = new Set(['.git', 'node_modules', 'target', 'dist', '.bmad'])
const IMAGE_LINE = /^\s*(?:-\s*)?image:\s*(.+?)\s*$/
const FROM_LINE = /^\s*FROM\s+(.+?)\s*$/i
const WORD_RE = new RegExp(`\\b${FORBIDDEN}\\b`, 'i')

function repoRoot() {
  const arg = process.argv[2]
  if (arg) return resolve(arg)
  const here = dirname(fileURLToPath(import.meta.url)) // pult/platform/scripts
  return resolve(here, '..', '..', '..') // корень репозитория
}

function listScanFiles(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue
      listScanFiles(join(dir, entry.name), out)
      continue
    }
    const name = entry.name
    if (name.endsWith('.yml') || name.endsWith('.yaml') || name.startsWith('Dockerfile')) {
      out.push(join(dir, name))
    }
  }
  return out
}

const root = repoRoot()
if (!existsSync(root)) {
  console.error('[AD-28] valkey-grep: корень не найден:', root)
  process.exit(2)
}

const violations = []

for (const file of listScanFiles(root)) {
  const lines = readFileSync(file, 'utf8').split('\n')
  lines.forEach((line, idx) => {
    const isDockerfile = basename(file).startsWith('Dockerfile')
    const match = isDockerfile ? line.match(FROM_LINE) : line.match(IMAGE_LINE)
    if (!match) return
    const value = match[1].replace(/["']/g, '')
    if (WORD_RE.test(value)) {
      violations.push({ file: file.slice(root.length + 1), line: idx + 1, raw: line.trim() })
    }
  })
}

if (violations.length > 0) {
  console.error('[AD-28] valkey-grep: ЗАПРЕЩЁННЫЙ КЭШ-ОБРАЗ — сервер кэша только Valkey (valkey/valkey)')
  for (const v of violations) {
    console.error(`  - ${v.file}:${v.line}: ${v.raw}`)
  }
  console.error(`Итого нарушений: ${violations.length}`)
  process.exit(1)
}

console.log('[AD-28] valkey-grep: PASS — запрещённых образов кэша не найдено')
