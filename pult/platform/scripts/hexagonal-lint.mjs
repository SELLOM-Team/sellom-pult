#!/usr/bin/env node
/**
 * Гейт гексагона (AD-1): крейты каталога core не зависят и не импортируют
 * conveyor и shells.
 *
 * Проверяет:
 *  1. Манифесты Cargo.toml каждого крейта core — отсутствие зависимостей на
 *     pult-conveyor-* и shell-крейты (pult-shell-*), включая dev/build.
 *  2. Все исходники .rs крейтов core — отсутствие use и extern crate путей к
 *     ним (и вообще любого упоминания идентификаторов pult_conveyor_* и pult_shell_*).
 *
 * При нарушении перечисляет нарушившие крейты и завершается с ненулевым кодом.
 *
 * Запуск: node platform/scripts/hexagonal-lint.mjs [корень pult]
 */

import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// Конвейер и оболочки (включая шлюз — composition root), а также их возможные
// будущие crate-имена — запрещены ядру. path-формы ловят переименованные
// path-dep ключи (adapters = { path = "../../conveyor/adapters" }).
const FORBIDDEN_TOML_MARKERS = [
  'pult-conveyor-',
  'pult-shell-',
  'pult-gateway',
  '/gateway',
  'conveyor/',
  'shells/',
]
const FORBIDDEN_RS_IDENTIFIERS = [
  'pult_conveyor',
  'pult_shell',
  'pult_gateway',
  'gateway::',
  'conveyor/',
  'shells/',
  'shells::',
]

function pultRoot() {
  const arg = process.argv[2]
  if (arg) return resolve(arg)
  const here = dirname(fileURLToPath(import.meta.url)) // pult/platform/scripts
  return resolve(here, '..', '..') // pult/
}

function listCrateDirs(coreDir) {
  return readdirSync(coreDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()
}

function listRsFiles(dir) {
  const out = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) out.push(...listRsFiles(full))
    else if (entry.name.endsWith('.rs')) out.push(full)
  }
  return out
}

const root = pultRoot()
const coreDir = join(root, 'core')
if (!existsSync(coreDir)) {
  console.error('[AD-1] hexagonal-lint: не найден каталог core/ в', root)
  process.exit(2)
}

const crates = listCrateDirs(coreDir)
if (crates.length === 0) {
  console.error('[AD-1] hexagonal-lint: в core/ нет ни одного крейта — гейту нечего проверять')
  process.exit(2)
}

// Комментарии в манифесте не должны триггерить гейт (# до конца строки).
function stripTomlComments(text) {
  return text
    .split('\n')
    .map((line) => line.replace(/#.*$/, ''))
    .join('\n')
}

// Постатейные комментарии .rs (//, ///, //!) в начале строки — не код:
// doc-комментарий, объясняющий правило AD-1, не нарушение. Внутристрочные
// // не срезаются (консервативно для запрещающего гейта).
function stripFullLineRsComments(source) {
  return source
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n')
}

// Чтение с честным кодом: ошибка скана (права/битый symlink) ≠ нарушение.
function readOrDie(file, gateLabel) {
  try {
    return readFileSync(file, 'utf8')
  } catch (err) {
    console.error(`[${gateLabel}] ошибка чтения ${file}: ${err.message}`)
    process.exit(2)
  }
}

const violations = []

// 0. Корневой workspace-манифест: [workspace.dependencies] с path на
// conveyor//shells//gateway даёт ядру алиас-обход ({ workspace = true }) —
// корень не должен реэкспортировать запрещённые пути.
const rootManifestPath = join(root, 'Cargo.toml')
if (existsSync(rootManifestPath)) {
  const rootManifest = stripTomlComments(readOrDie(rootManifestPath, 'AD-1'))
  const workspaceSection = rootManifest.split('[workspace.dependencies]')[1]
  if (workspaceSection) {
    // Секция кончается на следующей [таблице] или конец файла.
    const sectionBody = workspaceSection.split(/^\[.*\]$/m)[0]
    for (const marker of ['conveyor/', 'shells/', '/gateway', 'pult-gateway']) {
      if (sectionBody.includes(marker)) {
        violations.push({
          crate: '(workspace)',
          file: 'Cargo.toml',
          detail: `[workspace.dependencies] ведёт на запрещённый путь («${marker}») — алиас-обход границы ядра`,
        })
      }
    }
  }
}

for (const crate of crates) {
  const crateDir = join(coreDir, crate)

  // 1. Манифест: зависимости от конвейера/оболочек/шлюза запрещены.
  const manifestPath = join(crateDir, 'Cargo.toml')
  if (existsSync(manifestPath)) {
    const manifest = stripTomlComments(readOrDie(manifestPath, 'AD-1'))
    for (const marker of FORBIDDEN_TOML_MARKERS) {
      if (manifest.includes(marker)) {
        violations.push({ crate, file: `core/${crate}/Cargo.toml`, detail: `зависимость/упоминание «${marker}»` })
      }
    }
  }

  // 2. Исходники: импорты и любые пути к конвейеру/оболочкам/шлюзу запрещены.
  const srcDir = join(crateDir, 'src')
  if (existsSync(srcDir)) {
    for (const file of listRsFiles(srcDir)) {
      const source = stripFullLineRsComments(readOrDie(file, 'AD-1'))
      for (const identifier of FORBIDDEN_RS_IDENTIFIERS) {
        if (source.includes(identifier)) {
          violations.push({
            crate,
            file: file.slice(root.length + 1),
            detail: `использование «${identifier}» (use/путь к конвейеру, оболочке или шлюзу)`,
          })
        }
      }
    }
  }
}

if (violations.length > 0) {
  console.error('[AD-1] hexagonal-lint: НАРУШЕНИЕ ГРАНИЦЫ ЯДРА — core/* не может зависеть от conveyor/*, shells/* и gateway')
  for (const v of violations) {
    console.error(`  - крейт ${v.crate === '(workspace)' ? 'workspace' : `pult-core-${v.crate}`}: ${v.file} — ${v.detail}`)
  }
  console.error(`Итого нарушений: ${violations.length}`)
  process.exit(1)
}

console.log(`[AD-1] hexagonal-lint: PASS — граница ядра чиста (${crates.length} крейтов core/*)`)
