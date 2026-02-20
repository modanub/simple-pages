document.addEventListener('DOMContentLoaded', () => {
    const uploadZone = document.getElementById('upload-zone');
    const fileInput = document.getElementById('file-input');
    const uploadProgress = document.getElementById('upload-progress');
    const progressBar = document.getElementById('progress-bar');
    const uploadStatus = document.getElementById('upload-status');
    const uploadResult = document.getElementById('upload-result');
    const deleteBtn = document.getElementById('delete-btn');

    if (!uploadZone) return;

    // Load initial site info
    loadSiteInfo();

    // Click to browse
    uploadZone.addEventListener('click', () => fileInput.click());

    // Drag & drop
    uploadZone.addEventListener('dragover', (e) => {
        e.preventDefault();
        uploadZone.classList.add('dragover');
    });

    uploadZone.addEventListener('dragleave', () => {
        uploadZone.classList.remove('dragover');
    });

    uploadZone.addEventListener('drop', (e) => {
        e.preventDefault();
        uploadZone.classList.remove('dragover');
        if (e.dataTransfer.files.length > 0) {
            uploadFile(e.dataTransfer.files[0]);
        }
    });

    // File input change
    fileInput.addEventListener('change', () => {
        if (fileInput.files.length > 0) {
            uploadFile(fileInput.files[0]);
        }
    });

    // Delete button
    if (deleteBtn) {
        deleteBtn.addEventListener('click', async () => {
            if (!confirm('Delete all your site files? This cannot be undone.')) return;
            deleteBtn.classList.add('is-loading');
            try {
                const res = await fetch('/api/site', { method: 'DELETE' });
                const data = await res.json();
                if (data.success) {
                    loadSiteInfo();
                    uploadResult.innerHTML = '<div class="notification is-warning is-light">All files deleted.</div>';
                    uploadResult.style.display = '';
                }
            } catch (e) {
                alert('Failed to delete files');
            } finally {
                deleteBtn.classList.remove('is-loading');
            }
        });
    }

    async function uploadFile(file) {
        const validTypes = ['.zip', '.tar.gz', '.tgz'];
        const isValid = validTypes.some(ext => file.name.toLowerCase().endsWith(ext));
        if (!isValid) {
            alert('Please upload a .zip or .tar.gz file');
            return;
        }

        uploadProgress.style.display = '';
        uploadResult.style.display = 'none';
        uploadStatus.textContent = `Uploading ${file.name}...`;
        progressBar.removeAttribute('value');

        const formData = new FormData();
        formData.append('file', file);

        try {
            const xhr = new XMLHttpRequest();
            xhr.open('POST', '/api/site/upload');

            xhr.upload.addEventListener('progress', (e) => {
                if (e.lengthComputable) {
                    const pct = Math.round((e.loaded / e.total) * 100);
                    progressBar.value = pct;
                    uploadStatus.textContent = `Uploading... ${pct}%`;
                }
            });

            const result = await new Promise((resolve, reject) => {
                xhr.onload = () => {
                    try {
                        resolve({ status: xhr.status, data: JSON.parse(xhr.responseText) });
                    } catch {
                        reject(new Error('Invalid response'));
                    }
                };
                xhr.onerror = () => reject(new Error('Network error'));
                xhr.send(formData);
            });

            uploadProgress.style.display = 'none';

            if (result.status === 200 && result.data.success) {
                uploadResult.innerHTML = `
                    <div class="notification is-success is-light">
                        Upload successful! <a href="${result.data.site_url}" target="_blank">View your site</a>
                    </div>`;
            } else {
                uploadResult.innerHTML = `
                    <div class="notification is-danger is-light">
                        ${result.data.error || 'Upload failed'}
                    </div>`;
            }
            uploadResult.style.display = '';
            loadSiteInfo();
        } catch (e) {
            uploadProgress.style.display = 'none';
            uploadResult.innerHTML = `<div class="notification is-danger is-light">Upload failed: ${e.message}</div>`;
            uploadResult.style.display = '';
        }
    }

    async function loadSiteInfo() {
        try {
            const res = await fetch('/api/site');
            if (!res.ok) return;
            const data = await res.json();

            // Update quota
            const usageMB = (data.disk_usage_bytes / (1024 * 1024)).toFixed(1);
            const quotaMB = (data.quota_bytes / (1024 * 1024)).toFixed(0);
            const pct = data.quota_bytes > 0 ? (data.disk_usage_bytes / data.quota_bytes * 100) : 0;

            document.getElementById('usage-text').textContent = `${usageMB} MB / ${quotaMB} MB`;
            const quotaBar = document.getElementById('quota-bar');
            quotaBar.value = pct;
            quotaBar.max = 100;

            if (pct > 90) quotaBar.classList.replace('is-info', 'is-danger');
            else if (pct > 70) quotaBar.classList.replace('is-info', 'is-warning');

            // Update file list
            const fileList = document.getElementById('file-list');
            if (data.files.length === 0) {
                fileList.innerHTML = '<p class="has-text-grey">No files uploaded yet.</p>';
            } else {
                let html = '<table class="table is-fullwidth is-narrow"><thead><tr><th>File</th><th>Size</th></tr></thead><tbody>';
                for (const f of data.files) {
                    const size = f.size < 1024 ? `${f.size} B`
                        : f.size < 1048576 ? `${(f.size / 1024).toFixed(1)} KB`
                        : `${(f.size / 1048576).toFixed(1)} MB`;
                    html += `<tr><td><code>${f.path}</code></td><td>${size}</td></tr>`;
                }
                html += '</tbody></table>';
                fileList.innerHTML = html;
            }
        } catch (e) {
            console.error('Failed to load site info:', e);
        }
    }
});
