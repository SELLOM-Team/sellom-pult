#!/usr/bin/env node
/**
 * Гейт AD-30: бюджет производительности фронтенда — суммарный gzip размер
 * JS-ассетов входных (initial) чанков SPA должен быть < лимита (200 000 байт).
 *
 * Входные чанки определяются по dist/index.html: `<script type="module" src=...>`
 * и `<link rel="modulepreload" href=...>` — это критический путь; ленивые
 * чанки (dynamic import) в бюджет не входят.
 *
 * Запуск: node platform/scripts/bundle-size.mjs <dist> <лимит-байт>
 * Пример: node platform/scripts/bundle-size.mjs pult/shells/bitrix24/dist 200000
 */

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { gzipSync } from 'node:zlib'

function fail(message, code = 1) {
  console.error(`[AD-30] bundle-size: ${message}`)
  process.exit(code)
}

const [distArg, limitArg] = process.argv.slice(2)
if (!distArg || !limitArg) {
  fail('использование: node bundle-size.mjs <dist> <лимит-байт>', 2)
}
const dist = resolve(distArg)
const limit = Number(limitArg)
if (!Number.isFinite(limit) || limit <= 0) {
  fail(`лимит невалиден: ${limitArg}`, 2)
}
const indexHtmlPath = join(dist, 'index.html')
if (!existsSync(indexHtmlPath)) {
  fail(`dist не найден или index.html отсутствует: ${indexHtmlPath} (сначала npm run build)`, 2)
}

const html = readFileSync(indexHtmlPath, 'utf8')

// <script type="module" ... src="..."> в любом порядке атрибутов.
const initialRefs = new Set()
for (const tagMatch of html.matchAll(/<script\b[^>]*>/g)) {
  const tag = tagMatch[0]
  if (!/type\s*=\s*["']module["']/.test(tag)) continue
  const src = tag.match(/src\s*=\s*["']([^"']+)["']/)
  if (src) initialRefs.add(src[1])
}
// <link rel="modulepreload" href="...">
for (const linkMatch of html.matchAll(/<link\b[^>]*>/g)) {
  const tag = linkMatch[0]
  if (!/rel\s*=\s*["']modulepreload["']/.test(tag)) continue
  const href = tag.match(/href\s*=\s*["']([^"']+)["']/)
  if (href) initialRefs.add(href[1])
}

let jsFiles = [...initialRefs]
  .filter((ref) => ref.endsWith('.js'))
  .map((ref) => join(dist, ref.replace(/^\//, '')))
  .filter((path) => existsSync(path))

if (jsFiles.length === 0) {
  // Fallback: нет модульных ссылок — берём все JS в dist/assets (честно предупреждаем).
  const assetsDir = join(dist, 'assets')
  if (existsSync(assetsDir)) {
    jsFiles = readdirSync(assetsDir)
      .filter((name) => name.endsWith('.js'))
      .map((name) => join(assetsDir, name))
    console.warn('[AD-30] bundle-size: в index.html нет module-скриптов, измеряю все assets/*.js')
  }
}

if (jsFiles.length === 0) {
  fail('в dist не найдено JS-ассетов входных чанков', 2)
}

let total = 0
console.log(`[AD-30] bundle-size: входных JS-чанков ${jsFiles.length}, лимит ${limit} байт (gzip)`)
for (const file of jsFiles) {
  const gzipped = gzipSync(readFileSync(file)).length
  total += gzipped
  const rel = file.slice(dist.length + 1)
  console.log(`  ${rel} → ${gzipped} байт (gzip, raw ${statSync(file).size})`)
}

console.log(`Итого критический путь: ${total} / ${limit} байт (gzip)`)

if (total >= limit) {
  fail(`FAIL — бюджет превышен: ${total} ≥ ${limit} байт (gzip)`, 1)
}

console.log('PASS — бюджет критического пути соблюдён')
