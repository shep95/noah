import { NextRequest, NextResponse } from 'next/server'

interface GithubFile {
  name: string
  path: string
  type: 'file' | 'dir'
  download_url?: string
  sha: string
}

async function fetchTree(
  owner: string,
  repo: string,
  token: string,
  treeSha: string
): Promise<GithubFile[]> {
  const res = await fetch(
    `https://api.github.com/repos/${owner}/${repo}/git/trees/${treeSha}?recursive=1`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      },
    }
  )
  if (!res.ok) throw new Error(`GitHub tree fetch failed: ${res.status}`)
  const data = await res.json()
  return data.tree || []
}

async function fetchFileContent(url: string, token: string): Promise<string> {
  const res = await fetch(url, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  })
  if (!res.ok) return ''
  return res.text()
}

export async function POST(req: NextRequest) {
  try {
    const { owner, repo, token, branch = 'main' } = await req.json()

    if (!owner || !repo || !token) {
      return NextResponse.json(
        { error: 'owner, repo, and token are required' },
        { status: 400 }
      )
    }

    // Get repo info + default branch
    const repoRes = await fetch(`https://api.github.com/repos/${owner}/${repo}`, {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      },
    })

    if (!repoRes.ok) {
      const msg = repoRes.status === 404 ? 'Repository not found' : 'GitHub API error'
      return NextResponse.json({ error: msg }, { status: repoRes.status })
    }

    const repoData = await repoRes.json()
    const defaultBranch = branch || repoData.default_branch || 'main'

    // Get branch head commit
    const branchRes = await fetch(
      `https://api.github.com/repos/${owner}/${repo}/branches/${defaultBranch}`,
      {
        headers: {
          Authorization: `Bearer ${token}`,
          Accept: 'application/vnd.github+json',
          'X-GitHub-Api-Version': '2022-11-28',
        },
      }
    )

    if (!branchRes.ok) {
      return NextResponse.json({ error: 'Branch not found' }, { status: 404 })
    }

    const branchData = await branchRes.json()
    const treeSha = branchData.commit.commit.tree.sha

    const treeItems = await fetchTree(owner, repo, token, treeSha)

    // Filter to relevant code files (skip binaries, node_modules, .git)
    const CODE_EXTENSIONS = new Set([
      'ts', 'tsx', 'js', 'jsx', 'py', 'rs', 'go', 'java', 'cpp', 'c', 'h',
      'cs', 'php', 'rb', 'swift', 'kt', 'vue', 'svelte', 'astro', 'css',
      'scss', 'sass', 'less', 'html', 'json', 'yaml', 'yml', 'toml', 'md',
      'txt', 'sh', 'bash', 'env', 'gitignore', 'dockerfile',
    ])

    const SKIP_DIRS = new Set([
      'node_modules', '.git', '.next', 'dist', 'build', '__pycache__',
      '.pytest_cache', 'target', 'vendor', 'venv', '.venv', 'coverage',
    ])

    const filteredFiles = treeItems.filter((item) => {
      if (item.type === 'tree') return false
      const parts = item.path.split('/')
      if (parts.some((p) => SKIP_DIRS.has(p))) return false
      const ext = item.path.split('.').pop()?.toLowerCase() || ''
      return CODE_EXTENSIONS.has(ext)
    })

    // Limit to first 100 files to avoid overloading
    const filesToFetch = filteredFiles.slice(0, 100)

    // Fetch file contents in parallel (batches of 10)
    const files: { path: string; content: string; language: string }[] = []
    for (let i = 0; i < filesToFetch.length; i += 10) {
      const batch = filesToFetch.slice(i, i + 10)
      const contents = await Promise.all(
        batch.map(async (f) => {
          const url = `https://raw.githubusercontent.com/${owner}/${repo}/${defaultBranch}/${f.path}`
          const content = await fetchFileContent(url, token)
          const ext = f.path.split('.').pop()?.toLowerCase() || 'text'
          return { path: f.path, content: content.slice(0, 8000), language: ext }
        })
      )
      files.push(...contents)
    }

    return NextResponse.json({
      name: repoData.name,
      description: repoData.description,
      language: repoData.language,
      files,
      fileCount: files.length,
      totalFiles: treeItems.filter((i) => i.type === 'blob').length,
    })
  } catch (err) {
    console.error('GitHub route error:', err)
    return NextResponse.json({ error: 'Failed to import repository' }, { status: 500 })
  }
}
