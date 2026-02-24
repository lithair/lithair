// Load mermaid from CDN and initialize with theme detection
(function () {
    var script = document.createElement('script');
    script.src = 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js';
    script.onload = function () {
        var darkThemes = ['ayu', 'navy', 'coal'];
        var lightThemes = ['light', 'rust'];
        var classList = document.getElementsByTagName('html')[0].classList;

        var lastThemeWasLight = true;
        for (var i = 0; i < classList.length; i++) {
            if (darkThemes.indexOf(classList[i]) !== -1) {
                lastThemeWasLight = false;
                break;
            }
        }

        var theme = lastThemeWasLight ? 'default' : 'dark';
        mermaid.initialize({ startOnLoad: true, theme: theme });

        for (var d = 0; d < darkThemes.length; d++) {
            var el = document.getElementById(darkThemes[d]);
            if (el) el.addEventListener('click', function () {
                if (lastThemeWasLight) window.location.reload();
            });
        }

        for (var l = 0; l < lightThemes.length; l++) {
            var el = document.getElementById(lightThemes[l]);
            if (el) el.addEventListener('click', function () {
                if (!lastThemeWasLight) window.location.reload();
            });
        }
    };
    document.head.appendChild(script);
})();
