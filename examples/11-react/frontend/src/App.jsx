import { useState, useEffect } from "react";

const API = "/api/notes";

export default function App() {
  const [notes, setNotes] = useState([]);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");

  async function fetchNotes() {
    const res = await fetch(API);
    if (res.ok) setNotes(await res.json());
  }

  useEffect(() => {
    fetchNotes();
  }, []);

  async function addNote(e) {
    e.preventDefault();
    if (!title.trim()) return;
    await fetch(API, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title, content, completed: false }),
    });
    setTitle("");
    setContent("");
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

  return (
    <>
      <h1>Notes (React)</h1>

      <form onSubmit={addNote}>
        <input
          placeholder="Title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <textarea
          placeholder="Content"
          value={content}
          onChange={(e) => setContent(e.target.value)}
        />
        <button type="submit">Add Note</button>
      </form>

      <ul>
        {notes.map((note) => (
          <li key={note.id} className={note.completed ? "completed" : ""}>
            <div className="note-text">
              <h3>{note.title}</h3>
              <p>{note.content}</p>
            </div>
            <div className="note-actions">
              <button className="secondary" onClick={() => toggleNote(note)}>
                {note.completed ? "Undo" : "Done"}
              </button>
              <button className="danger" onClick={() => deleteNote(note.id)}>
                Del
              </button>
            </div>
          </li>
        ))}
      </ul>
    </>
  );
}
