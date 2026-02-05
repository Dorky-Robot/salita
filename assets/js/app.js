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
