import apiClient from './client'

/** 网盘账号（脱敏视图，不含凭据密文） */
export interface PanAccount {
  id: number
  platform: string // quark | uc | baidu
  display_name: string
  status: string // active | disabled | expired
  target_dir: string
  capacity_bytes: number | null
  last_checked_at: string | null
  created_at: string
  updated_at: string
}

export interface CreatePanAccount {
  platform: string
  display_name: string
  credential: string
  target_dir: string
}

export interface UpdatePanAccount {
  display_name?: string
  credential?: string
  target_dir?: string
}

export async function listPanAccounts(): Promise<PanAccount[]> {
  const res = await apiClient.get('/pan/accounts')
  return res.data.data
}

export async function createPanAccount(data: CreatePanAccount): Promise<PanAccount> {
  const res = await apiClient.post('/pan/accounts', data)
  return res.data.data
}

export async function updatePanAccount(
  id: number,
  data: UpdatePanAccount,
): Promise<PanAccount> {
  const res = await apiClient.put(`/pan/accounts/${id}`, data)
  return res.data.data
}

export async function deletePanAccount(id: number): Promise<void> {
  await apiClient.delete(`/pan/accounts/${id}`)
}

/** 手动健康校验（回写 status/capacity） */
export async function checkPanAccount(id: number): Promise<PanAccount> {
  const res = await apiClient.post(`/pan/accounts/${id}/check`)
  return res.data.data
}

/** 转存/上传任务 */
export interface TransferTask {
  id: number
  source_url: string
  source_type: string // pan_share | direct_link
  source_platform: string | null
  extract_code: string | null
  target_account_id: number
  status: string // pending | processing | succeeded | failed
  failure_reason: string | null
  share_id: number | null
  source_origin: string
  retry_count: number
  created_at: string
  started_at: string | null
  completed_at: string | null
  // 详情接口附加
  share_url?: string | null
  share_extract_code?: string | null
}

export async function listTransferTasks(params: {
  status?: string
  account_id?: number
  page?: number
  page_size?: number
}): Promise<{ items: TransferTask[]; total: number; page: number; page_size: number }> {
  const res = await apiClient.get('/pan/transfers', { params })
  return res.data.data
}

export async function getTransferTask(id: number): Promise<TransferTask> {
  const res = await apiClient.get(`/pan/transfers/${id}`)
  return res.data.data
}

export async function createTransfer(data: {
  source_url: string
  extract_code?: string
  target_account_id: number
}): Promise<TransferTask> {
  const res = await apiClient.post('/pan/transfers', data)
  return res.data.data
}

export async function retryTransfer(id: number): Promise<TransferTask> {
  const res = await apiClient.post(`/pan/transfers/${id}/retry`)
  return res.data.data
}
