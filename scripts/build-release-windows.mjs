#!/usr/bin/env node
// Windows 双架构发布（NSIS 安装包）：arm64 + x64 —— macOS 交叉编译变体。
// 对应 fund01 仓库 scripts/build-release-windows.mjs 的 macOS 版（那边是 Windows 上
// vcvarsall.bat + MSVC；这边是 cargo-xwin 下载 Windows SDK/CRT + clang 交叉编译）。
// 本仓库 demo 前端是静态资源（frontendDist: ../src），无需 Node 构建步骤。
//
// 原理（Tauri 2 官方支持）：cargo-xwin 拉取 Windows SDK/CRT 到本地缓存（首次约 1GB，
// 可设 XWIN_CACHE_DIR 共享），clang-cl 交叉编译 MSVC 目标，再用本机 makensis（brew nsis）
// 出 NSIS 安装包。MSI/WiX 不支持交叉编译，故仅 --bundles nsis。
//
// 用法（仓库根目录，仅限 macOS 运行）：
//   node scripts/build-release-windows.mjs               # arm64 + x64 都打
//   node scripts/build-release-windows.mjs --arch x64    # 仅 x64（别名 x86_64）
//   node scripts/build-release-windows.mjs --arch arm64  # 仅 ARM64（别名 aarch64）
// 可选环境变量：
//   TASKBAND_RELEASE_DIR  归档目录（默认 <repo>/release-windows）
//   TASKBAND_EXE_SUFFIX   归档文件名后缀（如 -ci：multiline-taskband-demo_1.0.0_x64-ci-setup.exe）
//   XWIN_CACHE_DIR        cargo-xwin 的 SDK 缓存目录（默认 cargo-xwin 自管，~/.cache/cargo-xwin）
//
// 前置（一次性）：
//   brew install nsis llvm
//   rustup target add aarch64-pc-windows-msvc x86_64-pc-windows-msvc
//   cargo install --locked cargo-xwin
//   cd examples/demo && pnpm install   # 装 @tauri-apps/cli
import { execFileSync, execSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const tauriConfPath = path.join(root, 'examples/demo/src-tauri/tauri.conf.json')
const demoDir = path.join(root, 'examples/demo')

if (process.platform !== 'darwin') {
  console.error('此脚本只能在 macOS 上运行（cargo-xwin 交叉编译 + 本机 makensis）')
  process.exit(1)
}

const ARCHES = {
  arm64: { target: 'aarch64-pc-windows-msvc', label: 'Windows on ARM' },
  x64: { target: 'x86_64-pc-windows-msvc', label: 'Windows x64 (Intel/AMD)' },
}
const ALIASES = { aarch64: 'arm64', x86_64: 'x64' }

const releaseDir = process.env.TASKBAND_RELEASE_DIR || path.join(root, 'release-windows')
const exeSuffix = process.env.TASKBAND_EXE_SUFFIX || ''

function run(cmd, extraEnv) {
  console.log(`\n▶ ${cmd}`)
  execSync(cmd, { stdio: 'inherit', cwd: root, env: extraEnv })
}

// 解析 --arch 参数（支持 --arch=x64 与 --arch x64 两种写法）
const archArg = process.argv.find((a) => a.startsWith('--arch='))
  || (process.argv.includes('--arch') ? process.argv[process.argv.indexOf('--arch') + 1] : null)
const archKey = archArg ? ALIASES[archArg] || archArg : null
const selected = archKey ? { [archKey]: ARCHES[archKey] } : ARCHES
if (!Object.keys(selected).length || Object.values(selected).some((v) => !v)) {
  console.error('未知架构，可选：arm64 / x64')
  process.exit(1)
}

// 版本号从 tauri.conf.json 读（与 tauri CLI 安装包命名口径一致）
const version = JSON.parse(readFileSync(tauriConfPath, 'utf-8')).version
if (!version) {
  console.error(`无法从 ${tauriConfPath} 读取 version`)
  process.exit(1)
}

// 前置检查：给出可执行的修复提示而不是跑一半才失败
function checkPreconditions() {
  const missing = []
  const tryWhich = (bin) => {
    try {
      execFileSync('which', [bin], { encoding: 'utf8' })
      return true
    } catch {
      return false
    }
  }
  if (!tryWhich('cargo-xwin')) missing.push('cargo install --locked cargo-xwin')
  if (!tryWhich('makensis')) missing.push('brew install nsis')
  if (!existsSync(demoDir)) {
    console.error('找不到 examples/demo，请在仓库根目录运行')
    process.exit(1)
  }
  if (!existsSync(path.join(demoDir, 'node_modules/@tauri-apps/cli'))) {
    missing.push('cd examples/demo && pnpm install')
  }
  if (missing.length) {
    console.error('缺少前置依赖，请先执行：\n  ' + missing.join('\n  '))
    process.exit(1)
  }
}

// brew llvm 的 clang-cl 前置到 PATH（cargo-xwin 需要；系统 clang 不带 MSVC 兼容驱动）
const brewLlvmBin = '/opt/homebrew/opt/llvm/bin'
const hasBrewLlvm = existsSync(path.join(brewLlvmBin, 'clang-cl'))
const buildEnv = {
  ...process.env,
  PATH: `${hasBrewLlvm ? `${brewLlvmBin}:` : ''}${process.env.PATH || ''}`,
}

checkPreconditions()
console.log(`归档目录: ${releaseDir}`)
if (hasBrewLlvm) console.log(`LLVM: ${brewLlvmBin}`)
else console.log('警告: 未找到 brew llvm 的 clang-cl，若构建失败请 brew install llvm')

mkdirSync(releaseDir, { recursive: true })

const archived = []
for (const [arch, { target, label }] of Object.entries(selected)) {
  console.log(`\n========== 构建 ${label} 版 (${target}) ==========`)
  run(
    `cd ${demoDir} && npx tauri build --runner cargo-xwin --target ${target} --bundles nsis`,
    buildEnv,
  )

  const bundleDir = path.join(root, `examples/demo/src-tauri/target/${target}/release/bundle/nsis`)
  // tauri 命名口径：multiline-taskband-demo_<version>_<arch>-setup.exe
  // （VM 时代 ARM64 会误标 _x64；macOS host 上 tauri CLI 命名正确，无需改名）
  const srcExe = path.join(bundleDir, `multiline-taskband-demo_${version}_${arch}-setup.exe`)
  if (!existsSync(srcExe)) {
    console.error(`构建产物缺失：${srcExe}（检查 tauri.conf.json version 与构建输出）`)
    process.exit(1)
  }

  const dstName = `multiline-taskband-demo_${version}_${arch}${exeSuffix}-setup.exe`
  const dstExe = path.join(releaseDir, dstName)
  copyFileSync(srcExe, dstExe)
  // 裸 exe 一并归档（dist/ 交付惯例：_x64/_arm64 后缀裸 exe + setup）
  const srcBare = path.join(root, `examples/demo/src-tauri/target/${target}/release/multiline-taskband-demo.exe`)
  const dstBare = path.join(releaseDir, `multiline-taskband-demo_${arch}.exe`)
  copyFileSync(srcBare, dstBare)
  archived.push(dstExe, dstBare)
  console.log(`\n✓ 已归档：${dstExe}\n          ${dstBare}`)
}

// 重新生成 SHA256SUMS.txt（覆盖归档目录里现有全部安装包，sha256sum 格式）
const sumLines = readdirSync(releaseDir)
  .filter((f) => f.endsWith('-setup.exe'))
  .sort()
  .map((f) => {
    const hash = createHash('sha256').update(readFileSync(path.join(releaseDir, f))).digest('hex')
    return `${hash}  ${f}`
  })
writeFileSync(path.join(releaseDir, 'SHA256SUMS.txt'), `${sumLines.join('\n')}\n`, 'utf-8')

console.log('\n========== build-release-windows 完成 ==========')
for (const f of archived) console.log(`  ${f}`)
console.log(`  ${path.join(releaseDir, 'SHA256SUMS.txt')}`)
