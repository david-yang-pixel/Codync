import { expect, test } from '@playwright/test'
import { api, attach } from './sandbox.ts'

type SavedBot = { id: string; backend: string; model?: string; loginProfile?: string }

test('new bots use the Codex default and the agent picker binds Claude Magpie to its profile', async () => {
  const { browser, page } = await attach()
  const name = `Magpie settings ${Date.now()}`
  const bots: string[] = []
  let profile: string | undefined
  try {
    const create = (settings: Partial<SavedBot>) =>
      api<{ bot: SavedBot }>(page, 'createBot', { name, connectors: [], skills: [], ...settings })
    const { bot: defaults } = await api<{ bot: SavedBot }>(page, 'createBot', { name: `${name} default`, connectors: [], skills: [] })
    bots.push(defaults.id)
    expect(defaults.backend).toBe('codex')
    expect(defaults.model ?? null).toBeNull()

    const { bot: legacy } = await create({ backend: 'claude', model: 'opus' })
    bots.push(legacy.id)
    expect(legacy).toMatchObject({ backend: 'claude', model: 'opus' })
    await expect(create({ backend: 'claude-magpie' })).rejects.toThrow()

    const { loginProfile } = await api<{ loginProfile: { id: string } }>(page, 'addLoginProfile', { backend: 'claude', kind: 'apiKey' })
    profile = loginProfile.id
    await api(page, 'renameLoginProfile', { backend: 'claude', loginProfile: profile, name: 'Magpie (Codync)' })
    await api(page, 'setLoginProfileKeys', {
      loginProfile: profile,
      vars: { ANTHROPIC_BASE_URL: 'http://127.0.0.1:12345', ANTHROPIC_AUTH_TOKEN: 'fixture-token', ANTHROPIC_MODEL: 'fixture-model' },
    })

    await page.reload()
    await page.locator('.roster-row').filter({ has: page.getByText(name, { exact: true }) }).click()
    await page.getByRole('button', { name: `${name}, view details` }).click()
    const details = page.locator('.details-panel')
    await details.getByRole('tab', { name: 'Settings' }).click()
    const picker = details.locator('.agent-picker').first()
    await picker.locator('.agent-picker-summary').click()
    await picker.locator('.agent-picker-choice').first().click()
    await expect(picker.locator('.agent-picker-item').filter({ hasText: /Claude Code/ })).toHaveCount(0)
    const magpie = picker.getByRole('button', { name: 'Claude Magpie', exact: true })
    await expect(magpie).toBeVisible()
    await magpie.click()
    await expect(picker.locator('.agent-picker-summary')).toContainText('Default · Login profile')
    await expect(picker.getByRole('textbox', { name: 'Model', exact: true })).toHaveCount(0)
    await expect.poll(async () => {
      const { bot } = await api<{ bot: SavedBot }>(page, 'updateBot', { id: legacy.id })
      return [bot.backend, bot.loginProfile]
    }).toEqual(['claude-magpie', profile])
  } finally {
    for (const botId of bots) await api(page, 'deleteBot', { botId }).catch(() => {})
    if (profile) await api(page, 'removeLoginProfile', { loginProfile: profile }).catch(() => {})
    await browser.close()
  }
})
