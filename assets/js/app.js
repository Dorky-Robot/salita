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
