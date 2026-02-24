<script setup>
import { ref, onMounted } from "vue";

const API = "/api/notes";
const notes = ref([]);
const title = ref("");
const content = ref("");

async function fetchNotes() {
  const res = await fetch(API);
  if (res.ok) notes.value = await res.json();
}

async function addNote() {
  if (!title.value.trim()) return;
  await fetch(API, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title: title.value,
      content: content.value,
      completed: false,
    }),
  });
  title.value = "";
  content.value = "";
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

onMounted(fetchNotes);
</script>

<template>
  <h1>Notes (Vue)</h1>

  <form @submit.prevent="addNote">
    <input v-model="title" placeholder="Title" />
    <textarea v-model="content" placeholder="Content" />
    <button type="submit">Add Note</button>
  </form>

  <ul>
    <li v-for="note in notes" :key="note.id" :class="{ completed: note.completed }">
      <div class="note-text">
        <h3>{{ note.title }}</h3>
        <p>{{ note.content }}</p>
      </div>
      <div class="note-actions">
        <button class="secondary" @click="toggleNote(note)">
          {{ note.completed ? "Undo" : "Done" }}
        </button>
        <button class="danger" @click="deleteNote(note.id)">Del</button>
      </div>
    </li>
  </ul>
</template>
