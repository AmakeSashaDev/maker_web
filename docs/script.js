const fallbackData = {
    downloads: 0,
    stars: 0,
    version: '0.1.0'
};

function formatNumber(num) {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'k';
    return num.toString();
}

function animateCounter(element, target, suffix = '') {
    let current = 0;
    const increment = target / 50;
    const timer = setInterval(() => {
        current += increment;
        if (current >= target) {
            element.textContent = formatNumber(target) + suffix;
            clearInterval(timer);
        } else {
            element.textContent = formatNumber(Math.floor(current)) + suffix;
        }
    }, 20);
}

async function getCrateData() {
    try {
        const response = await fetch('https://crates.io/api/v1/crates/maker_web');
        if (!response.ok) throw new Error('Crates API error');
        
        const data = await response.json();
        return {
            downloads: data.crate.downloads,
            version: data.crate.max_version
        };
    } catch (error) {
        console.warn('Using fallback data for crates.io');
        return {
            downloads: fallbackData.downloads,
            version: fallbackData.version
        };
    }
}

async function getGitHubStars() {
    try {
        const response = await fetch('https://api.github.com/repos/AmakeSashaDev/maker_web');
        
        if (response.status === 403) {
            return fallbackData.stars;
        }
        
        if (!response.ok) throw new Error('GitHub API error');
        
        const data = await response.json();
        return data.stargazers_count;
    } catch (error) {
        console.warn('Using fallback stars');
        return fallbackData.stars;
    }
}

async function loadStats() {
    try {
        const [crateData, stars] = await Promise.allSettled([
            getCrateData(),
            getGitHubStars()
        ]);
        
        const downloads = crateData.status === 'fulfilled' ? 
            crateData.value.downloads : fallbackData.downloads;
        const version = crateData.status === 'fulfilled' ? 
            crateData.value.version : fallbackData.version;
        const starCount = stars.status === 'fulfilled' ? 
            stars.value : fallbackData.stars;
        
        const downloadsEl = document.getElementById('downloads');
        const starsEl = document.getElementById('stars');
        const versionEl = document.getElementById('version');
        
        downloadsEl.innerHTML = '';
        starsEl.innerHTML = '';
        versionEl.innerHTML = '';
        
        animateCounter(downloadsEl, downloads);
        animateCounter(starsEl, starCount);
        versionEl.textContent = 'v' + version;
        
    } catch (error) {
        console.error('Error loading stats:', error);
        
        document.getElementById('downloads').textContent = formatNumber(fallbackData.downloads);
        document.getElementById('stars').textContent = formatNumber(fallbackData.stars);
        document.getElementById('version').textContent = 'v' + fallbackData.version;
    }
}

function getThemeFromURL() {
    const params = new URLSearchParams(window.location.search);
    return params.get('theme');
}

function setTheme(theme) {
    document.body.setAttribute('data-theme', theme);
}

function updateURLWithTheme(theme) {
    const url = new URL(window.location);
    url.searchParams.set('theme', theme);
    window.history.pushState({}, '', url);
}

function setupThemeToggle() {
    const themeToggle = document.getElementById('themeToggle');
    
    themeToggle.addEventListener('click', () => {
        const currentTheme = document.body.getAttribute('data-theme');
        const newTheme = currentTheme === 'light' ? 'dark' : 'light';
        
        setTheme(newTheme);
        updateURLWithTheme(newTheme);
        
        themeToggle.style.transform = 'scale(0.95)';
        setTimeout(() => {
            themeToggle.style.transform = '';
        }, 150);
    });
}

function initTheme() {
    const themeFromURL = getThemeFromURL();
    
    if (themeFromURL === 'dark') {
        setTheme('dark');
    } else {
        setTheme('light');
        
        if (!themeFromURL) {
            updateURLWithTheme('light');
        }
    }
}

function setupShareModal() {
    const shareLink = document.getElementById('shareLink');
    const shareOverlay = document.getElementById('shareOverlay');
    const copyButton = document.getElementById('copyButton');
    const closeButton = document.getElementById('closeShare');
    const shareUrl = document.getElementById('shareUrl');
    
    const projectUrl = 'https://github.com/AmakeSashaDev/maker_web';
    
    shareLink.addEventListener('click', (e) => {
        e.preventDefault();
        shareOverlay.style.display = 'flex';
        document.body.style.overflow = 'hidden';
        shareUrl.value = projectUrl;
        shareUrl.select();
    });
    
    copyButton.addEventListener('click', () => {
        navigator.clipboard.writeText(projectUrl).then(() => {
            copyButton.textContent = 'Copied!';
            copyButton.classList.add('copied');
            
            setTimeout(() => {
                copyButton.textContent = 'Copy';
                copyButton.classList.remove('copied');
            }, 2000);
        });
    });
    
    closeButton.addEventListener('click', () => {
        shareOverlay.style.display = 'none';
        document.body.style.overflow = 'auto';
    });
    
    shareOverlay.addEventListener('click', (e) => {
        if (e.target === shareOverlay) {
            shareOverlay.style.display = 'none';
            document.body.style.overflow = 'auto';
        }
    });
    
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && shareOverlay.style.display === 'flex') {
            shareOverlay.style.display = 'none';
            document.body.style.overflow = 'auto';
        }
    });
}

function setupDownloadModal() {
    const shareLink = document.getElementById('windowDownloadLink');
    const shareOverlay = document.getElementById('windowDownload');
    const closeButton = document.getElementById('closeWindowDownload');
    
    const projectUrl = 'https://github.com/AmakeSashaDev/maker_web';
    
    shareLink.addEventListener('click', (e) => {
        e.preventDefault();
        shareOverlay.style.display = 'flex';
        document.body.style.overflow = 'hidden';
        shareUrl.value = projectUrl;
        shareUrl.select();
    });
    
    closeButton.addEventListener('click', () => {
        shareOverlay.style.display = 'none';
        document.body.style.overflow = 'auto';
    });
    
    shareOverlay.addEventListener('click', (e) => {
        if (e.target === shareOverlay) {
            shareOverlay.style.display = 'none';
            document.body.style.overflow = 'auto';
        }
    });
    
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && shareOverlay.style.display === 'flex') {
            shareOverlay.style.display = 'none';
            document.body.style.overflow = 'auto';
        }
    });
}

function setupExamplesCopy() {
    document.querySelectorAll('.api-example pre').forEach(pre => {
        const copyBtn = document.createElement('button');
        copyBtn.className = 'copy-example-btn';
        copyBtn.innerHTML = '📋';
        copyBtn.title = 'Copy code';
        
        copyBtn.addEventListener('click', async () => {
            const code = pre.querySelector('code').textContent;
            try {
                await navigator.clipboard.writeText(code);
                copyBtn.textContent = '✅';
                setTimeout(() => {
                    copyBtn.innerHTML = '📋';
                }, 2000);
            } catch (err) {
                console.error('Copy error:', err);
            }
        });
        
        pre.appendChild(copyBtn);
    });
}

function init() {
    document.getElementById('currentYear').textContent = new Date().getFullYear();
    
    initTheme();
    setupThemeToggle();
    setupShareModal();
    setupDownloadModal();
    setupExamplesCopy();
    
    loadStats();
    setInterval(loadStats, 2 * 60 * 1000);
    
    window.addEventListener('popstate', () => {
        initTheme();
    });
}

document.addEventListener('DOMContentLoaded', init);