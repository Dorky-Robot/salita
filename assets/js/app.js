// Salita client-side JavaScript
// htmx handles most interactivity; this is for anything extra.

document.addEventListener('DOMContentLoaded', function() {
  // Flash message auto-dismiss
  document.querySelectorAll('[data-flash]').forEach(function(el) {
    setTimeout(function() {
      el.style.transition = 'opacity 0.3s';
      el.style.opacity = '0';
      setTimeout(function() { el.remove(); }, 300);
    }, 4000);
  });
});

// Modal close behavior with exit animation
function closeConnectModal(event) {
  if (event && event.target !== event.currentTarget) return;
  var overlay = document.getElementById('connect-modal');
  if (!overlay) return;
  overlay.classList.add('modal-overlay--closing');
  overlay.addEventListener('animationend', function() {
    var container = document.getElementById('modal-container');
    if (container) container.innerHTML = '';
  }, { once: true });
}

document.addEventListener('keydown', function(e) {
  if (e.key === 'Escape') {
    closeConnectModal();
  }
});

// Copy LAN URL to clipboard
function copyLanUrl(btn) {
  var url = btn.getAttribute('data-url');
  navigator.clipboard.writeText(url).then(function() {
    var icon = btn.querySelector('i');
    icon.className = 'ph ph-check';
    btn.classList.add('modal__url-copy--copied');
    setTimeout(function() {
      icon.className = 'ph ph-copy';
      btn.classList.remove('modal__url-copy--copied');
    }, 2000);
  });
}

// Character counter for post compose textarea
function updateCharCount(form) {
  var textarea = form.querySelector('textarea[name="body"]');
  var counter = form.querySelector('#char-count');
  if (!textarea || !counter) return;
  var remaining = 2000 - textarea.value.length;
  counter.textContent = remaining;
  if (remaining < 100) {
    counter.className = 'text-xs text-orange-500 font-medium';
  } else {
    counter.className = 'text-xs text-stone-400';
  }
}
