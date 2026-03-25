<script>
  const API = "/api/notes";

  let notes = $state([]);
  let title = $state("");
  let content = $state("");

  async function fetchNotes() {
    const res = await fetch(API);
    if (res.ok) notes = await res.json();
  }

  async function addNote(e) {
    e.preventDefault();
    if (!title.trim()) return;
    await fetch(API, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title, content, completed: false }),
    });
    title = "";
    content = "";
    fetchNotes();
  }

  async function toggleNote(note) {
    await fetch(`${API}/${note.id}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...note, completed: !note.completed }),
    });
    fetchNotes();
  }

  async function deleteNote(id) {
    await fetch(`${API}/${id}`, { method: "DELETE" });
    fetchNotes();
  }

  $effect(() => {
    fetchNotes();
  });
</script>

<h1>Notes (Svelte)</h1>

<form onsubmit={addNote}>
  <input bind:value={title} placeholder="Title" />
  <textarea bind:value={content} placeholder="Content"></textarea>
  <button type="submit">Add Note</button>
</form>

<ul>
  {#each notes as note (note.id)}
    <li class:completed={note.completed}>
      <div class="note-text">
        <h3>{note.title}</h3>
        <p>{note.content}</p>
      </div>
      <div class="note-actions">
        <button class="secondary" onclick={() => toggleNote(note)}>
          {note.completed ? "Undo" : "Done"}
        </button>
        <button class="danger" onclick={() => deleteNote(note.id)}>Del</button>
      </div>
    </li>
  {/each}
</ul>
